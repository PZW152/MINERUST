import java.io.*;
import java.util.*;
import com.google.common.collect.Multimap;
import net.minecraft.creativetab.CreativeTabs;
import net.minecraft.entity.ai.attributes.AttributeModifier;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.nbt.NBTTagList;

/**
 * Reference dump of the item registry for names, tooltips and the creative
 * inventory:
 *   N id meta tab key rarity attack|display name     (every sub-item)
 *   T index label                                     (creative tabs)
 *   C index id meta count ench...                     (displayAllReleventItems)
 *   S id meta count ench...                           (the search tab list)
 *   X id name maxLevel                                (enchantments)
 *   E id name                                         (spawn egg entities)
 *   R id name / B id name                             (item / block registry names)
 * ench is id:lvl pairs from ench / StoredEnchantments.
 *
 * Usage: ItemHarness <out>
 */
public class ItemHarness {
    static String ench(ItemStack s) {
        StringBuilder b = new StringBuilder();
        NBTTagCompound t = s.getTagCompound();
        if (t == null) return "";
        NBTTagList l = t.hasKey("ench") ? t.getTagList("ench", 10) : t.getTagList("StoredEnchantments", 10);
        for (int i = 0; i < l.tagCount(); i++) {
            NBTTagCompound e = l.getCompoundTagAt(i);
            b.append(' ').append(e.getShort("id")).append(':').append(e.getShort("lvl"));
        }
        return b.toString();
    }

    static String st(ItemStack s) {
        return Item.getIdFromItem(s.getItem()) + " " + s.getItemDamage() + " " + s.stackSize + ench(s);
    }

    /** getSubItems; BlockFlower counts its (client-only) icons instead. */
    static void subItems(Item it, CreativeTabs t, List list) {
        try {
            it.getSubItems(it, t, list);
        } catch (NullPointerException e) {
            int id = Item.getIdFromItem(it);
            int n = id == 38 ? 9 : id == 175 ? 6 : 1;
            for (int i = 0; i < n; i++) list.add(new ItemStack(it, 1, i));
            System.err.println("subitems fallback " + id);
        }
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        PrintWriter w = new PrintWriter(new OutputStreamWriter(new FileOutputStream(a[0]), "UTF-8"));
        for (Object o : Item.itemRegistry) {
            Item it = (Item) o;
            List list = new ArrayList();
            CreativeTabs tab = it.getCreativeTab();
            if (tab != null) subItems(it, null, list);
            if (list.isEmpty()) list.add(new ItemStack(it, 1, 0));
            for (Object so : list) {
                ItemStack s = (ItemStack) so;
                double atk = 0;
                Multimap m = s.getAttributeModifiers();
                for (Object e : m.entries()) {
                    Map.Entry en = (Map.Entry) e;
                    if ("generic.attackDamage".equals(en.getKey())) atk = ((AttributeModifier) en.getValue()).getAmount();
                }
                w.println("N " + Item.getIdFromItem(it) + " " + s.getItemDamage() + " "
                    + (tab == null ? -1 : tab.getTabIndex()) + " " + s.getUnlocalizedName() + " "
                    + s.getRarity().name() + " " + atk + "|" + s.getDisplayName());
            }
        }
        for (CreativeTabs t : CreativeTabs.creativeTabArray) {
            if (t == null) continue;
            w.println("T " + t.getTabIndex() + " " + t.getTabLabel());
            if (t == CreativeTabs.tabAllSearch || t == CreativeTabs.tabInventory) continue;
            List list = new ArrayList();
            for (Object o : Item.itemRegistry) {
                Item it = (Item) o;
                if (it != null && it.getCreativeTab() == t) subItems(it, t, list);
            }
            if (t.getRelevantEnchantmentTypes() != null) t.addEnchantmentBooksToList(list, t.getRelevantEnchantmentTypes());
            for (Object so : list) w.println("C " + t.getTabIndex() + " " + st((ItemStack) so));
        }
        List search = new ArrayList();
        for (Object o : Item.itemRegistry) {
            Item it = (Item) o;
            if (it != null && it.getCreativeTab() != null) subItems(it, null, search);
        }
        for (net.minecraft.enchantment.Enchantment e : net.minecraft.enchantment.Enchantment.enchantmentsList) {
            if (e != null && e.type != null) net.minecraft.init.Items.enchanted_book.getAll(e, search);
        }
        for (Object so : search) w.println("S " + st((ItemStack) so));
        for (Object o : Item.itemRegistry) {
            Item it = (Item) o;
            w.println("R " + Item.getIdFromItem(it) + " " + Item.itemRegistry.getNameForObject(it));
        }
        for (Object o : net.minecraft.block.Block.blockRegistry) {
            net.minecraft.block.Block b = (net.minecraft.block.Block) o;
            w.println("B " + net.minecraft.block.Block.getIdFromBlock(b) + " " + net.minecraft.block.Block.blockRegistry.getNameForObject(b));
        }
        for (net.minecraft.enchantment.Enchantment e : net.minecraft.enchantment.Enchantment.enchantmentsList) {
            if (e != null) w.println("X " + e.effectId + " " + e.getName() + " " + e.getMaxLevel());
        }
        for (Object k : net.minecraft.entity.EntityList.entityEggs.keySet()) {
            int id = (Integer) k;
            Object egg = net.minecraft.entity.EntityList.entityEggs.get(k);
            int pc = egg.getClass().getField("primaryColor").getInt(egg);
            int sc = egg.getClass().getField("secondaryColor").getInt(egg);
            w.println("E " + id + " " + net.minecraft.entity.EntityList.getStringFromID(id) + " " + pc + " " + sc);
        }
        w.close();
    }
}
