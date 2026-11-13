"""Generates src/item_data.rs from tools/items_1710.txt (ItemHarness)."""
import sys

src = sys.argv[1] if len(sys.argv) > 1 else 'tools/items_1710.txt'
out = sys.argv[2] if len(sys.argv) > 2 else 'src/item_data.rs'

RAR = {'common': 0, 'uncommon': 1, 'rare': 2, 'epic': 3}
names, tabs, creative, search, ench, eggs, english, reg, breg = [], [], [], [], [], [], [], [], []


def stack(f):
    e = []
    for p in f[3:]:
        a, b = p.split(':')
        e.append('(%s, %s)' % (a, b))
    return '%s, %s, %s, &[%s]' % (f[0], f[1], f[2], ', '.join(e))


for line in open(src, encoding='utf-8'):
    line = line.rstrip('\n')
    if not line:
        continue
    k, rest = line[0], line[2:]
    if k == 'N':
        head, disp = rest.split('|', 1)
        f = head.split()
        atk = float(f[5])
        names.append('(%s, %s, %s, "%s", %d, %s)' % (f[0], f[1], f[2], f[3], RAR[f[4]], repr(atk) + ('' if '.' in repr(atk) else '.0')))
        english.append('%s %s %s' % (f[0], f[1], disp))
    elif k == 'T':
        f = rest.split()
        tabs.append('"%s"' % f[1])
    elif k == 'C':
        f = rest.split()
        creative.append('(%s, %s)' % (f[0], stack(f[1:])))
    elif k == 'S':
        search.append('(%s)' % stack(rest.split()))
    elif k == 'X':
        f = rest.split()
        ench.append('(%s, "%s", %s)' % (f[0], f[1], f[2]))
    elif k == 'R':
        f = rest.split()
        reg.append('(%s, "%s")' % (f[0], f[1]))
    elif k == 'B':
        f = rest.split()
        breg.append('(%s, "%s")' % (f[0], f[1]))
    elif k == 'E':
        f = rest.split()
        eggs.append('(%s, "%s", %s, %s)' % (f[0], f[1], f[2], f[3]))

w = open(out, 'w', encoding='utf-8', newline='\n')
w.write('''//! The 1.7.10 item registry as `tools/ItemHarness.java` dumped it
//! (`tools/items_1710.txt`, regenerate with `tools/gen_items.py`): every
//! sub-item's unlocalized name, rarity and attack damage modifier, the
//! creative tabs and their contents, enchantments and spawn eggs.

#![allow(clippy::all)]

/// `(id, meta, creative tab or -1, getUnlocalizedName, EnumRarity, attack)`
pub const NAMES: &[(u16, i32, i8, &str, u8, f32)] = &[
''')
for n in names:
    w.write('    %s,\n' % n)
w.write('];\n\n/// `CreativeTabs.creativeTabArray` labels.\npub const TABS: [&str; %d] = [%s];\n\n' % (len(tabs), ', '.join(tabs)))
w.write('/// One stack: `(id, meta, count, enchantments)`.\npub type Entry = (u16, i32, u32, &\'static [(i32, i32)]);\n\n')
w.write('/// `CreativeTabs.displayAllReleventItems` per tab: `(tab, stack)`.\npub const CREATIVE: &[(u8, u16, i32, u32, &[(i32, i32)])] = &[\n')
for c in creative:
    w.write('    %s,\n' % c)
w.write('];\n\n/// `GuiContainerCreative.updateCreativeSearch` before filtering.\npub const SEARCH: &[Entry] = &[\n')
for s in search:
    w.write('    %s,\n' % s)
w.write('];\n\n/// `Enchantment.enchantmentsList`: `(id, getName, getMaxLevel)`.\npub const ENCHANTMENTS: &[(i32, &str, i32)] = &[\n')
for e in ench:
    w.write('    %s,\n' % e)
w.write('];\n\n/// `EntityList.entityEggs`: `(id, getStringFromID, primaryColor, secondaryColor)`.\npub const EGGS: &[(i32, &str, i32, i32)] = &[\n')
for e in eggs:
    w.write('    %s,\n' % e)
w.write('];\n\n/// `Item.itemRegistry` names.\npub const ITEM_NAMES: &[(u16, &str)] = &[\n')
for e in reg:
    w.write('    %s,\n' % e)
w.write('];\n\n/// `Block.blockRegistry` names.\npub const BLOCK_NAMES: &[(u16, &str)] = &[\n')
for e in breg:
    w.write('    %s,\n' % e)
w.write('];\n')
w.close()
open('tools/item_names_en.txt', 'w', encoding='utf-8', newline='\n').write('\n'.join(english) + '\n')
print(len(names), len(creative), len(search), len(ench), len(eggs))
