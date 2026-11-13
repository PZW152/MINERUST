import java.io.*;
import net.minecraft.entity.player.EntityPlayer;
import net.minecraft.init.Bootstrap;
import net.minecraft.inventory.Container;
import net.minecraft.inventory.InventoryCrafting;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.item.crafting.CraftingManager;

/**
 * Reference results of RecipesArmorDyes: each line is the grid
 * (id:meta[:color] ...) and the resulting display.color (or "none").
 */
public class DyeHarness {
    static ItemStack st(String s) {
        String[] f = s.split(":");
        ItemStack i = new ItemStack(Item.getItemById(Integer.parseInt(f[0])), 1, Integer.parseInt(f[1]));
        if (f.length > 2) ((net.minecraft.item.ItemArmor) i.getItem()).setColor(i, Integer.parseInt(f[2]));
        return i;
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        String[] cases = {
            "298:0 351:1", "299:0 351:1 351:4", "300:0:16711680 351:11", "301:0 351:15 351:0 351:5",
            "298:0 351:14 351:14 351:9", "299:0:255 351:2", "306:0 351:1", "298:0 298:0 351:1", "298:0",
        };
        PrintWriter w = new PrintWriter(new FileWriter(a[0]));
        for (String c : cases) {
            InventoryCrafting inv = new InventoryCrafting(new Container() {
                public boolean canInteractWith(EntityPlayer p) { return true; }
            }, 3, 3);
            String[] parts = c.split(" ");
            for (int i = 0; i < parts.length; i++) inv.setInventorySlotContents(i, st(parts[i]));
            ItemStack r = CraftingManager.getInstance().findMatchingRecipe(inv, null);
            String out = "none";
            if (r != null && r.hasTagCompound() && r.getTagCompound().getCompoundTag("display").hasKey("color"))
                out = "" + r.getTagCompound().getCompoundTag("display").getInteger("color");
            else if (r != null) out = "item" + Item.getIdFromItem(r.getItem());
            w.println(c + "|" + out);
        }
        w.close();
    }
}
