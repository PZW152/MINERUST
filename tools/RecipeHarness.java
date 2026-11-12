import java.io.*;
import java.lang.reflect.Field;
import java.util.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.item.crafting.CraftingManager;
import net.minecraft.item.crafting.FurnaceRecipes;
import net.minecraft.item.crafting.IRecipe;
import net.minecraft.item.crafting.ShapedRecipes;
import net.minecraft.item.crafting.ShapelessRecipes;

/**
 * Reference dump of CraftingManager.getRecipeList(), in its order:
 *   S w h out_id out_meta out_count id:meta ...   (shaped, '-' for empty)
 *   L out_id out_meta out_count id:meta ...       (shapeless)
 *   X ClassName                                   (special recipe classes)
 *
 * Usage: RecipeHarness <out>
 */
public class RecipeHarness {
    static String st(ItemStack s) {
        if (s == null) return "-";
        return Item.getIdFromItem(s.getItem()) + ":" + s.getItemDamage();
    }

    static String out(ItemStack s) {
        return Item.getIdFromItem(s.getItem()) + " " + s.getItemDamage() + " " + s.stackSize;
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        PrintWriter w = new PrintWriter(new FileWriter(a[0]));
        List list = CraftingManager.getInstance().getRecipeList();
        for (Object o : list) {
            IRecipe r = (IRecipe) o;
            Class<?> c = r.getClass();
            if (c == ShapedRecipes.class) {
                int[] ints = new int[2];
                int k = 0;
                ItemStack[] items = null;
                ItemStack res = null;
                for (Field f : ShapedRecipes.class.getDeclaredFields()) {
                    f.setAccessible(true);
                    if (f.getType() == int.class && k < 2) ints[k++] = f.getInt(r);
                    else if (f.getType() == ItemStack[].class) items = (ItemStack[]) f.get(r);
                    else if (f.getType() == ItemStack.class) res = (ItemStack) f.get(r);
                }
                StringBuilder b = new StringBuilder("S " + ints[0] + " " + ints[1] + " " + out(res));
                for (ItemStack s : items) b.append(' ').append(st(s));
                w.println(b);
            } else if (c == ShapelessRecipes.class) {
                List items = null;
                ItemStack res = null;
                for (Field f : ShapelessRecipes.class.getDeclaredFields()) {
                    f.setAccessible(true);
                    if (List.class.isAssignableFrom(f.getType())) items = (List) f.get(r);
                    else if (f.getType() == ItemStack.class) res = (ItemStack) f.get(r);
                }
                StringBuilder b = new StringBuilder("L " + out(res));
                for (Object s : items) b.append(' ').append(st((ItemStack) s));
                w.println(b);
            } else {
                w.println("X " + c.getSimpleName());
            }
        }
        w.close();
    }
}
