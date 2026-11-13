import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.entity.player.EntityPlayerMP;
import net.minecraft.entity.player.PlayerCapabilities;
import net.minecraft.init.Bootstrap;
import net.minecraft.init.Items;
import net.minecraft.inventory.ContainerRepair;
import net.minecraft.inventory.IInventory;
import net.minecraft.inventory.InventoryCraftResult;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.nbt.NBTTagList;

/**
 * Reference results of ContainerRepair.updateRepairOutput. Input lines in
 * tools/repair_cases.txt: in0|in1|name|creative, a stack written as
 * id:meta:count[:rc=N][:ench=id.lvl+id.lvl][:name=Text], "-" for none.
 * Output: the case, then "cost use id meta count rc name ench..." or
 * "cost none".
 */
public class RepairHarness {
    static ItemStack st(String s) {
        if (s.equals("-")) return null;
        String[] f = s.split(":");
        ItemStack i = new ItemStack(Item.getItemById(Integer.parseInt(f[0])), Integer.parseInt(f[2]), Integer.parseInt(f[1]));
        for (int k = 3; k < f.length; k++) {
            if (f[k].startsWith("rc=")) i.setRepairCost(Integer.parseInt(f[k].substring(3)));
            if (f[k].startsWith("name=")) i.setStackDisplayName(f[k].substring(5));
            if (f[k].startsWith("ench=")) for (String e : f[k].substring(5).split("[+]")) {
                String[] p = e.split("[.]");
                Enchantment en = Enchantment.enchantmentsList[Integer.parseInt(p[0])];
                if (i.getItem() == Items.enchanted_book) Items.enchanted_book.addEnchantment(i, new net.minecraft.enchantment.EnchantmentData(en, Integer.parseInt(p[1])));
                else i.addEnchantment(en, Integer.parseInt(p[1]));
            }
        }
        return i;
    }

    static Object unsafe() throws Exception {
        Field f = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        f.setAccessible(true);
        return f.get(null);
    }

    static Object alloc(Class<?> c) throws Exception {
        Object u = unsafe();
        return u.getClass().getMethod("allocateInstance", Class.class).invoke(u, c);
    }

    static void set(Object o, String name, Object v) throws Exception {
        Field f = ContainerRepair.class.getDeclaredField(name);
        f.setAccessible(true);
        f.set(o, v);
    }

    static Object get(Object o, String name) throws Exception {
        Field f = ContainerRepair.class.getDeclaredField(name);
        f.setAccessible(true);
        return f.get(o);
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        BufferedReader in = new BufferedReader(new FileReader(a[0]));
        PrintWriter w = new PrintWriter(new FileWriter(a[1]));
        String line;
        while ((line = in.readLine()) != null) {
            if (line.isEmpty() || line.startsWith("#")) continue;
            String[] f = line.split("[|]", -1);
            ContainerRepair c = (ContainerRepair) alloc(ContainerRepair.class);
            Class<?> k1 = Class.forName("net.minecraft.inventory.ContainerRepair$1");
            Constructor<?> ctor = k1.getDeclaredConstructors()[0];
            ctor.setAccessible(true);
            IInventory inputs = (IInventory) ctor.newInstance(c, "Repair", true, 2);
            set(c, "inputSlots", inputs);
            set(c, "outputSlot", new InventoryCraftResult());
            for (String fl : new String[]{"inventorySlots", "inventoryItemStacks", "crafters"}) {
                Field ff = net.minecraft.inventory.Container.class.getDeclaredField(fl);
                ff.setAccessible(true);
                ff.set(c, new ArrayList<Object>());
            }
            Field pc = net.minecraft.inventory.Container.class.getDeclaredField("playerList");
            pc.setAccessible(true);
            pc.set(c, new HashSet<Object>());
            EntityPlayerMP p = (EntityPlayerMP) alloc(EntityPlayerMP.class);
            PlayerCapabilities caps = new PlayerCapabilities();
            caps.isCreativeMode = f[3].equals("1");
            Field cf = net.minecraft.entity.player.EntityPlayer.class.getDeclaredField("capabilities");
            cf.setAccessible(true);
            cf.set(p, caps);
            set(c, "thePlayer", p);
            set(c, "repairedItemName", f[2]);
            // InventoryBasic.setInventorySlotContents without the change hook
            Field items = net.minecraft.inventory.InventoryBasic.class.getDeclaredField("inventoryContents");
            items.setAccessible(true);
            ItemStack[] arr = (ItemStack[]) items.get(inputs);
            arr[0] = st(f[0]);
            arr[1] = st(f[1]);
            c.updateRepairOutput();
            ItemStack out = ((IInventory) get(c, "outputSlot")).getStackInSlot(0);
            StringBuilder b = new StringBuilder(line + " => " + c.maximumCost);
            if (out == null) b.append(" none");
            else {
                b.append(" ").append(get(c, "stackSizeToBeUsedInRepair")).append(" ").append(Item.getIdFromItem(out.getItem()))
                    .append(" ").append(out.getItemDamage()).append(" ").append(out.stackSize).append(" ").append(out.getRepairCost())
                    .append(" ").append(out.hasDisplayName() ? out.getDisplayName().replace(' ', '_') : "-");
                NBTTagList l = out.getItem() == Items.enchanted_book ? Items.enchanted_book.getEnchantments(out) : out.getEnchantmentTagList();
                if (l != null) for (int i = 0; i < l.tagCount(); i++) {
                    NBTTagCompound e = l.getCompoundTagAt(i);
                    b.append(" ").append(e.getShort("id")).append(":").append(e.getShort("lvl"));
                }
            }
            w.println(b);
        }
        w.close();
    }
}
