"""Typed re-emission of a (Rust-ish) Java expression with Java's numeric
promotion made explicit as Rust casts.

Types: 'i32', 'f32', 'f64', 'bool', 'Icon', '?' (unknown, left alone).
"""
import re

RANK = {'i32': 0, 'f32': 1, 'f64': 2}

TOK = re.compile(r'\s*(?:(\d+\.\d+(?:f32|f64|F|D)?|\d+(?:f32|f64|F|L)?)|([A-Za-z_][\w.]*)|(<<|>>|<=|>=|==|!=|&&|\|\||[-+*/%&|^!<>()?:,]))')

CALL_TYPES = {
    'self.mixed': 'i32', 'self.ao_light': 'f32', 'self.can_block_grass': 'bool',
    'self.is_opaque': 'bool', 'self.meta_at': 'i32', 'self.should_side': 'bool',
    'self.icon': 'Icon', 'self.getAoBrightness': 'i32', 'self.mixAoBrightness': 'i32',
    'self.is_grass_top': 'bool', 'self.grass_overlay': 'Icon', 'self.block_at': 'i32',
}
METHOD_TYPES = {'u': 'f64', 'v': 'f64', 'min_u': 'f64', 'max_u': 'f64', 'min_v': 'f64', 'max_v': 'f64', 'is_some': 'bool'}


def tokenize(s):
    pos, out = 0, []
    s = s.strip()
    while pos < len(s):
        m = TOK.match(s, pos)
        if not m or m.end() == pos:
            raise ValueError('bad token at %r' % s[pos:])
        if m.group(1):
            out.append(('num', m.group(1)))
        elif m.group(2):
            out.append(('id', m.group(2)))
        else:
            out.append(('op', m.group(3)))
        pos = m.end()
    return out


BIN = [
    ['||'], ['&&'], ['|'], ['^'], ['&'], ['==', '!='], ['<', '>', '<=', '>='],
    ['<<', '>>'], ['+', '-'], ['*', '/', '%'],
]


class P:
    def __init__(self, toks, env):
        self.t = toks
        self.i = 0
        self.env = env

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else (None, None)

    def eat(self, v=None):
        tk = self.t[self.i]
        if v is not None and tk[1] != v:
            raise ValueError('expected %s got %s' % (v, tk))
        self.i += 1
        return tk

    def parse(self):
        return self.ternary()

    def ternary(self):
        c = self.binary(0)
        if self.peek()[1] == '?':
            self.eat('?')
            a = self.ternary()
            self.eat(':')
            b = self.ternary()
            t = promote(a[1], b[1])
            return ('if %s { %s } else { %s }' % (c[0], cast(a, t), cast(b, t)), t)
        return c

    def binary(self, lvl):
        if lvl == len(BIN):
            return self.unary()
        left = self.binary(lvl + 1)
        while self.peek()[0] == 'op' and self.peek()[1] in BIN[lvl]:
            op = self.eat()[1]
            right = self.binary(lvl + 1)
            left = combine(left, op, right)
        return left

    def unary(self):
        tk = self.peek()
        if tk[1] == '!':
            self.eat()
            a = self.unary()
            return ('!' + a[0], 'bool')
        if tk[1] == '-':
            self.eat()
            a = self.unary()
            return ('-' + a[0], a[1])
        if tk[1] == '(':
            # cast?
            if self.i + 2 < len(self.t) and self.t[self.i + 1][1] in ('float', 'int', 'double') and self.t[self.i + 2][1] == ')':
                self.eat('(')
                ty = {'float': 'f32', 'int': 'i32', 'double': 'f64'}[self.eat()[1]]
                self.eat(')')
                a = self.unary()
                return ('(%s as %s)' % (a[0], ty), ty)
            self.eat('(')
            a = self.parse()
            self.eat(')')
            return self.postfix(('(%s)' % a[0], a[1]))
        return self.postfix(self.primary())

    def primary(self):
        kind, v = self.eat()
        if kind == 'num':
            if v.endswith('F') or v.endswith('f32'):
                return (v.rstrip('F').replace('f32', '') + 'f32', 'f32')
            if v.endswith('f64') or v.endswith('D'):
                return (v.rstrip('D').replace('f64', '') + 'f64', 'f64')
            if '.' in v:
                return (v + 'f64', 'f64')
            return (v, 'i32')
        if kind == 'id':
            if self.peek()[1] == '(':
                self.eat('(')
                args = []
                while self.peek()[1] != ')':
                    args.append(self.parse())
                    if self.peek()[1] == ',':
                        self.eat(',')
                self.eat(')')
                base = v
                # method on a value: a.b(...)
                if '.' in v and not v.startswith('self.'):
                    obj, meth = v.rsplit('.', 1)
                    t = METHOD_TYPES.get(meth, '?')
                    return ('%s.%s(%s)' % (obj, meth, ', '.join(argcast(meth, a) for a in args)), t)
                if v.startswith('self.') and v.count('.') == 2:
                    obj, meth = v.rsplit('.', 1)
                    return ('%s.%s(%s)' % (obj, meth, ', '.join(a[0] for a in args)), METHOD_TYPES.get(meth, '?'))
                t = CALL_TYPES.get(v, '?')
                return ('%s(%s)' % (v, ', '.join(argcast(v, a, k) for k, a in enumerate(args))), t)
            t = self.env.get(v, '?')
            return (v, t)
        raise ValueError('unexpected %s' % v)

    def postfix(self, a):
        return a


def argcast(fn, a, k=0):
    # face and coordinate arguments
    if fn in ('u', 'v'):
        return cast(a, 'f64')
    if fn in ('self.mixed', 'self.ao_light', 'self.can_block_grass', 'self.is_opaque', 'self.meta_at', 'self.should_side', 'self.icon', 'self.block_at'):
        return cast(a, 'i32')
    if fn == 'self.getAoBrightness':
        return cast(a, 'i32')
    if fn == 'self.mixAoBrightness':
        return cast(a, 'i32' if k < 4 else 'f64')
    return a[0]


def promote(a, b):
    if a in RANK and b in RANK:
        return a if RANK[a] >= RANK[b] else b
    if a == '?':
        return b
    return a


def cast(a, t):
    if a[1] == t or a[1] not in RANK or t not in RANK:
        return a[0]
    if re.fullmatch(r'-?\d+', a[0]) and t in ('f32', 'f64'):
        return a[0] + '.0' + t
    return '(%s as %s)' % (a[0], t)


def combine(l, op, r):
    if op in ('&&', '||'):
        return ('%s %s %s' % (l[0], op, r[0]), 'bool')
    if op in ('==', '!=', '<', '>', '<=', '>='):
        t = promote(l[1], r[1])
        return ('%s %s %s' % (cast(l, t), op, cast(r, t)), 'bool')
    if op in ('<<', '>>'):
        return ('%s %s %s' % (l[0], op, r[0]), l[1])
    if op in ('&', '|', '^'):
        if l[1] == 'bool':
            return ('%s %s %s' % (l[0], op, r[0]), 'bool')
        return ('%s %s %s' % (l[0], op, r[0]), 'i32')
    t = promote(l[1], r[1])
    return ('%s %s %s' % (cast(l, t), op, cast(r, t)), t)


def typed(s, env, want=None):
    toks = tokenize(s)
    p = P(toks, env)
    e = p.parse()
    if p.i != len(toks):
        raise ValueError('trailing tokens in %r' % s)
    if want:
        # Java assignment: implicit widening only; explicit narrowing in source
        return cast(e, want)
    return e[0]
