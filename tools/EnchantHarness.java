import java.io.*;
import java.util.*;
import net.minecraft.enchantment.EnchantmentData;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;

/**
 * Reference dump of EnchantmentHelper.buildEnchantmentList and
 * calcItemStackEnchantability: per item id, level and Random seed the
 * (id:level) list, and the three table levels for 0..15 bookshelves.
 *   L item level seed id:lvl ...
 *   C item shelves seed a b c
 */
public class EnchantHarness {
    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        PrintWriter w = new PrintWriter(new FileWriter(a[0]));
        int[] items = {276, 257, 261, 310, 299, 301, 340, 346, 359, 283, 290};
        for (int id : items) {
            ItemStack s = new ItemStack(Item.getItemById(id));
            for (int level = 1; level <= 30; level += 7) {
                for (long seed = 0; seed < 6; seed++) {
                    List l = EnchantmentHelper.buildEnchantmentList(new Random(seed), s, level);
                    StringBuilder b = new StringBuilder("L " + id + " " + level + " " + seed);
                    if (l != null) for (Object o : l) {
                        EnchantmentData d = (EnchantmentData) o;
                        b.append(' ').append(d.enchantmentobj.effectId).append(':').append(d.enchantmentLevel);
                    }
                    w.println(b);
                }
            }
            for (int shelves = 0; shelves <= 16; shelves += 4) {
                Random r = new Random(shelves * 31 + id);
                StringBuilder b = new StringBuilder("C " + id + " " + shelves + " " + (shelves * 31 + id));
                for (int k = 0; k < 3; k++) b.append(' ').append(EnchantmentHelper.calcItemStackEnchantability(r, k, shelves, s));
                w.println(b);
            }
        }
        w.close();
    }
}
