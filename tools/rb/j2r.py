"""Mechanical Java -> Rust translation of RenderBlocks methods.

Usage: j2r.py RenderBlocks.java out.rs jname:rname:ret:sig ...
Only the statement forms these methods use are supported. Java's numeric
promotion is made explicit (see jexpr.py).
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from jexpr import typed, RANK  # noqa: E402

src = open(sys.argv[1], encoding='utf-8').read().split('\n')


def find_method(name):
    pat = re.compile(r'^   (public|private|protected) [\w<>\[\]]+ ' + re.escape(name) + r'\(')
    for i, l in enumerate(src):
        if pat.match(l):
            j = i
            while not src[j].rstrip().endswith('{'):
                j += 1
            depth = 0
            body = []
            for k in range(j, len(src)):
                depth += src[k].count('{') - src[k].count('}')
                body.append(src[k])
                if depth == 0:
                    return src[i:j + 1], body[1:-1]
    raise SystemExit('no method ' + name)


TYPES = {'float': 'f32', 'int': 'i32', 'double': 'f64', 'boolean': 'bool', 'IIcon': 'Icon'}

FIELDS = {}
for f in ['renderMinX', 'renderMaxX', 'renderMinY', 'renderMaxY', 'renderMinZ', 'renderMaxZ']:
    FIELDS['self.' + f] = 'f64'
for l in src[50:140]:
    m = re.match(r'\s+(?:private|public) (float|int|boolean|double) (\w+);', l)
    if m:
        FIELDS['self.' + m.group(2)] = TYPES[m.group(1)]
FIELDS['self.bright'] = 'i32'
FIELDS['FANCY_GRASS'] = 'bool'


def args_split(s):
    out, depth, cur = [], 0, ''
    for ch in s:
        if ch in '([':
            depth += 1
        elif ch in ')]':
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur.strip())
            cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def call_sub(s, prefix, fn):
    start = 0
    while True:
        i = s.find(prefix, start)
        if i < 0:
            return s
        j = i + len(prefix)
        depth = 1
        k = j
        while depth:
            if s[k] == '(':
                depth += 1
            elif s[k] == ')':
                depth -= 1
            k += 1
        a = args_split(s[j:k - 1])
        rep = fn(a)
        s = s[:i] + rep + s[k:]
        start = i + 1


def text(s):
    """Name-level replacements; the result is parsed by jexpr."""
    s = call_sub(s, 'this.blockAccess.getBlock(', lambda a: 'BLOCKAT(%s)' % ', '.join(a))
    s = re.sub(r'BLOCKAT\(([^()]*(?:\([^()]*\))?[^()]*)\)\.getAmbientOcclusionLightValue\(\)', r'self.ao_light(\1)', s)
    s = re.sub(r'BLOCKAT\(([^()]*(?:\([^()]*\))?[^()]*)\)\.getCanBlockGrass\(\)', r'self.can_block_grass(\1)', s)
    s = re.sub(r'BLOCKAT\(([^()]*(?:\([^()]*\))?[^()]*)\)\.isOpaqueCube\(\)', r'self.is_opaque(\1)', s)
    s = call_sub(s, 'this.blockAccess.getBlockMetadata(', lambda a: 'self.meta_at(%s)' % ', '.join(a))
    s = call_sub(s, 'var1.getMixedBrightnessForBlock(', lambda a: 'self.mixed(%s)' % ', '.join(a[1:]))
    s = call_sub(s, 'var1.shouldSideBeRendered(', lambda a: 'self.should_side(%s)' % ', '.join(a[1:]))
    s = s.replace('this.getBlockIcon(var1).getIconName().equals("grass_top")', 'self.is_grass_top()')
    s = call_sub(s, 'this.getBlockIcon(var1, this.blockAccess, ', lambda a: 'self.icon(%s)' % ', '.join(a))
    s = s.replace('this.hasOverrideBlockTexture()', 'self.overrideBlockTexture.is_some()')
    s = re.sub(r'(var\d+)\.getIconName\(\)\.equals\("grass_side"\)', r'\1.grass_side', s)
    s = s.replace('BlockGrass.getIconSideOverlay()', 'self.grass_overlay()')
    s = re.sub(r'\bfancyGrass\b', 'FANCY_GRASS', s)
    s = re.sub(r'(var\d+)\.getInterpolatedU\(', r'\1.u(', s)
    s = re.sub(r'(var\d+)\.getInterpolatedV\(', r'\1.v(', s)
    s = re.sub(r'(var\d+)\.getMinU\(\)', r'\1.min_u()', s)
    s = re.sub(r'(var\d+)\.getMaxU\(\)', r'\1.max_u()', s)
    s = re.sub(r'(var\d+)\.getMinV\(\)', r'\1.min_v()', s)
    s = re.sub(r'(var\d+)\.getMaxV\(\)', r'\1.max_v()', s)
    s = re.sub(r'\bthis\.', 'self.', s)
    return s


class Ctx:
    def __init__(self, params):
        self.env = dict(FIELDS)
        self.env.update(params)

    def ty(self, name):
        return self.env.get(name, '?')

    def ex(self, s, want=None):
        t = text(s)
        if t.endswith('.grass_side') and re.fullmatch(r'[\w.]+', t):
            return t
        try:
            return typed(t, self.env, want)
        except Exception as e:  # noqa: BLE001
            return '/*UNTYPED %s*/ %s' % (e, t)

    def args(self, s, tys):
        a = args_split(text(s))
        out = []
        for k, x in enumerate(a):
            want = tys[k] if k < len(tys) else None
            if x in ('self.grass_overlay()',) or re.fullmatch(r'var\d+', x) and self.ty(x) == 'Icon':
                out.append(x)
            elif x.startswith('self.icon('):
                out.append(typed(x, self.env))
            else:
                out.append(typed(x, self.env, want))
        return ', '.join(out)

    def stmt(self, l):
        t = l.strip()
        if not t:
            return []
        if re.match(r'Tessellator var\d+ = Tessellator\.instance;', t):
            return []
        m = re.match(r'(\w+)\.setColorOpaque_F\((.*)\);$', t)
        if m:
            return ['self.color(%s);' % self.args(m.group(2), ['f32'] * 3)]
        m = re.match(r'(\w+)\.setBrightness\((.*)\);$', t)
        if m:
            return ['self.bright = %s;' % self.ex(m.group(2), 'i32')]
        m = re.match(r'(\w+)\.addVertexWithUV\((.*)\);$', t)
        if m:
            return ['self.vertex(%s);' % self.args(m.group(2), ['f64'] * 5)]
        m = re.match(r'this\.renderFace(YNeg|YPos|ZNeg|ZPos|XNeg|XPos)\(var1, (.*)\);$', t)
        if m:
            f = {'YNeg': 'y_neg', 'YPos': 'y_pos', 'ZNeg': 'z_neg', 'ZPos': 'z_pos', 'XNeg': 'x_neg', 'XPos': 'x_pos'}[m.group(1)]
            return ['self.face_%s(%s);' % (f, self.args(m.group(2), ['f64', 'f64', 'f64', None]))]
        m = re.match(r'(float|int|double|boolean|IIcon) (var\d+) = (.*);$', t)
        if m:
            ty = TYPES[m.group(1)]
            self.env[m.group(2)] = ty
            v = self.ex(m.group(3), ty if ty in RANK else None)
            return ['let mut %s: %s = %s;' % (m.group(2), ty, v)]
        m = re.match(r'(float|int|double|boolean) (var\d+);$', t)
        if m:
            self.env[m.group(2)] = TYPES[m.group(1)]
            return ['let mut %s: %s;' % (m.group(2), TYPES[m.group(1)])]
        m = re.match(r'(var\d+)(\+\+|--);$', t)
        if m:
            return ['%s %s= 1;' % (m.group(1), '+' if m.group(2) == '++' else '-')]
        m = re.match(r'(if|} else if) \((.*)\) \{$', t)
        if m:
            kw = 'if' if m.group(1) == 'if' else '} else if'
            return ['%s %s {' % (kw, self.ex(m.group(2)))]
        if t in ('} else {', '}', '{'):
            return [t]
        m = re.match(r'return (.*);$', t)
        if m:
            return ['return %s;' % self.ex(m.group(1))]
        m = re.match(r'^((?:[\w.]+ = )+)(.*);$', t)
        if m:
            targets = [text(x.strip()) for x in m.group(1).split(' = ') if x.strip()]
            ty = self.ty(targets[-1])
            val = self.ex(m.group(2), ty if ty in RANK else None)
            out = ['%s = %s;' % (targets[-1], val)]
            for k in range(len(targets) - 2, -1, -1):
                out.append('%s = %s;' % (targets[k], targets[k + 1]))
            return out
        m = re.match(r'^([\w.]+) (\*|\+|-|/)= (.*);$', t)
        if m:
            tgt = text(m.group(1))
            ty = self.ty(tgt)
            # Java: a op= b  is  a = (T)(a op b)
            v = typed('%s %s (%s)' % (tgt, m.group(2), text(m.group(3))), self.env)
            src_t = typed_type(tgt, m.group(2), text(m.group(3)), self.env)
            if src_t != ty and ty in RANK and src_t in RANK:
                v = '(%s) as %s' % (v, ty)
            return ['%s = %s;' % (tgt, v)]
        return ['/*TODO*/ ' + text(t)]


def typed_type(tgt, op, rhs, env):
    from jexpr import P, tokenize
    p = P(tokenize('%s %s (%s)' % (tgt, op, rhs)), env)
    return p.parse()[1]


def body_rs(body, params):
    c = Ctx(params)
    out = []
    buf = ''
    for l in body:
        s = l.strip()
        if not s:
            continue
        buf = (buf + ' ' + s).strip() if buf else s
        if buf.endswith(';') or buf.endswith('{') or buf.endswith('}'):
            out.extend(c.stmt(buf))
            buf = ''
    res, ind = [], 2
    for r in out:
        if r.startswith('}'):
            ind -= 1
        res.append('    ' * ind + r)
        if r.endswith('{'):
            ind += 1
    return '\n'.join(res)


out = []
for spec in sys.argv[3:]:
    jname, rname, ret, sig = spec.split(':', 3)
    params = {}
    for p in sig.split(','):
        m = re.match(r'\s*(?:mut )?(var\d+): (\w+)', p)
        if m:
            params[m.group(1)] = m.group(2)
    _, body = find_method(jname)
    out.append('    /// `RenderBlocks.%s`\n    #[allow(unused_assignments, unused_mut, unused_variables, unused_parens, clippy::all)]\n    pub fn %s(%s)%s {\n%s\n    }\n'
               % (jname, rname, sig, (' -> ' + ret) if ret else '', body_rs(body, params)))
open(sys.argv[2], 'w', encoding='utf-8').write('\n'.join(out))
