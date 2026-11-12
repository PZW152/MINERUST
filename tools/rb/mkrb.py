import re, sys, os
S = os.path.dirname(os.path.abspath(__file__))
gen = open(os.path.join(S, 'rb_gen.rs'), encoding='utf-8').read()
gen = re.sub(r'self\.bright = (self\.render\w+ [<>] [\d.]+) \? (var\d+) : (self\.mixed\([^;]*\));',
             r'self.bright = if \1 { \2 } else { \3 };', gen)
gen = gen.replace('= self.overrideBlockTexture;', '= self.overrideBlockTexture.unwrap();').replace('var1 != Blocks.grass', 'self.id != block::GRASS')
extra = ''
p = os.path.join(S, 'rb_extra.rs')
if os.path.exists(p):
    extra = open(p, encoding='utf-8').read()
head = open(os.path.join(S, 'rb_head.rs'), encoding='utf-8').read()
open('src/render/rb.rs', 'w', encoding='utf-8').write(head + gen + extra + '}\n')
m = 'src/render/mesher.rs'
s = open(m, encoding='utf-8').read()
if 'mod rb;' not in s:
    s = s.replace('pub struct Vertex {', '#[path = "rb.rs"]\npub(crate) mod rb;\n\n#[derive(Clone, Copy, Debug, Default)]\npub struct Vertex {', 1) if False else s.replace('\nconst W: usize = 48;', '\n#[path = "rb.rs"]\npub(crate) mod rb;\n\nconst W: usize = 48;', 1)
    open(m, 'w', encoding='utf-8').write(s)
print('ok')
