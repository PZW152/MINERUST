import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.entity.Entity;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.item.EntityXPOrb;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.world.World;
import net.minecraft.world.WorldProvider;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.WorldInfo;

/**
 * Reference block drops: Block.dropBlockAsItemWithChance(world, 0, 64, 0,
 * meta, 1.0, fortune) on a world whose rand is new Random(seed), recording
 * what it spawns (items, experience orbs; the final rand.nextInt checks that
 * every draw, the item positions included, happened in order), plus
 * canSilkHarvest and createStackedBlock. Output: tools/drops_1710.txt.
 */
public class DropHarness {
    /** A World that only records what is spawned. */
    public static class FakeWorld extends World {
        public List<String> log = new ArrayList<>();

        public FakeWorld() {
            super(null, null, (WorldProvider) null, null, null);
        }

        @Override
        protected IChunkProvider createChunkProvider() {
            return null;
        }

        @Override
        protected int getRenderDistanceChunks() {
            return 0;
        }

        @Override
        public Entity getEntityByID(int var1) {
            return null;
        }

        @Override
        public boolean spawnEntityInWorld(Entity e) {
            if (e instanceof EntityItem) {
                ItemStack s = ((EntityItem) e).getEntityItem();
                log.add("i" + Item.getIdFromItem(s.getItem()) + ":" + s.getItemDamage() + ":" + s.stackSize);
            } else if (e instanceof EntityXPOrb) {
                log.add("x" + ((EntityXPOrb) e).getXpValue());
            } else {
                log.add("e" + e.getClass().getSimpleName());
            }
            return true;
        }
    }

    static Object unsafe() throws Exception {
        Field f = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        f.setAccessible(true);
        return f.get(null);
    }

    static void put(Object o, Class<?> c, String name, Object v) throws Exception {
        Object u = unsafe();
        Field f = c.getDeclaredField(name);
        long off = (Long) u.getClass().getMethod("objectFieldOffset", Field.class).invoke(u, f);
        if (f.getType() == boolean.class)
            u.getClass().getMethod("putBoolean", Object.class, long.class, boolean.class).invoke(u, o, off, (Boolean) v);
        else
            u.getClass().getMethod("putObject", Object.class, long.class, Object.class).invoke(u, o, off, v);
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        Object u = unsafe();
        FakeWorld w = (FakeWorld) u.getClass().getMethod("allocateInstance", Class.class).invoke(u, FakeWorld.class);
        put(w, FakeWorld.class, "log", new ArrayList<String>());
        put(w, World.class, "isRemote", false);
        put(w, World.class, "provider", WorldProvider.getProviderForDimension(0));
        put(w, World.class, "worldInfo", new WorldInfo(new NBTTagCompound()));
        Method silk = Block.class.getDeclaredMethod("canSilkHarvest");
        silk.setAccessible(true);
        Method stacked = Block.class.getDeclaredMethod("createStackedBlock", int.class);
        stacked.setAccessible(true);
        PrintWriter out = new PrintWriter(new FileWriter(a[0]));
        for (int id = 1; id < 176; id++) {
            Block b = Block.getBlockById(id);
            if (b == null || Block.getIdFromBlock(b) != id) continue;
            String sk;
            try {
                sk = ((Boolean) silk.invoke(b)) ? "1" : "0";
            } catch (Throwable t) {
                sk = "E";
            }
            for (int meta = 0; meta < 16; meta++) {
                StringBuilder line = new StringBuilder(id + " " + meta + " silk=" + sk);
                try {
                    ItemStack s = (ItemStack) stacked.invoke(b, meta);
                    line.append(" st=").append(s == null || s.getItem() == null ? "-" : Item.getIdFromItem(s.getItem()) + ":" + s.getItemDamage());
                } catch (Throwable t) {
                    line.append(" st=E");
                }
                out.println(line);
                for (int fortune = 0; fortune < 4; fortune++) {
                    for (int seed = 0; seed < 4; seed++) {
                        w.rand = new Random(seed * 7919L + fortune * 31 + meta);
                        w.log.clear();
                        String r;
                        try {
                            b.dropBlockAsItemWithChance(w, 0, 64, 0, meta, 1.0F, fortune);
                            r = String.join(" ", w.log) + " |" + w.rand.nextInt();
                        } catch (Throwable t) {
                            r = "ERR " + t.getClass().getSimpleName();
                        }
                        out.println("  " + fortune + " " + seed + " " + r);
                    }
                }
            }
        }
        out.close();
    }
}
