import java.io.*;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnchantmentDurability;
import net.minecraft.enchantment.EnchantmentHelper;
import net.minecraft.enchantment.EnchantmentThorns;
import net.minecraft.entity.Entity;
import net.minecraft.entity.EntityLiving;
import net.minecraft.entity.EntityLivingBase;
import net.minecraft.entity.EnumCreatureAttribute;
import net.minecraft.entity.item.EntityItem;
import net.minecraft.entity.monster.*;
import net.minecraft.entity.passive.*;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.util.DamageSource;
import net.minecraft.util.EntityDamageSourceIndirect;
import net.minecraft.world.World;
import net.minecraft.world.WorldProvider;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.storage.WorldInfo;

/**
 * Reference values for the enchantment effects and mob loot:
 *   p <type> <level> <source> <calcModifierDamage>
 *   c <type> <level> <attribute> <calcDamageByCreature>
 *   u <item> <unbreaking> <uses> <seed> <itemDamage> <broke> |<next int>
 *   t <level> <seed> <shouldHit> <getDamage> |<next int>
 *   d <mob> <variant> <burning> <hit> <looting> <seed> <items...> |<next int>
 * where d is onDeath's loot: dropFewItems(hit, looting), then with hit
 * rand.nextInt(200) - looting < 5 for dropRareDrop. Output: tools/enchant_fx_1710.txt
 */
public class EnchantFxHarness {
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

        /** The local difficulty onSpawnWithEgg reads, fixed per case. */
        public float tension;

        @Override
        public float getTensionFactorForBlock(double x, double y, double z) {
            return tension;
        }

        @Override
        public boolean spawnEntityInWorld(Entity e) {
            if (e instanceof EntityItem) {
                ItemStack s = ((EntityItem) e).getEntityItem();
                log.add(Item.getIdFromItem(s.getItem()) + ":" + s.getItemDamage() + ":" + s.stackSize);
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
        else if (f.getType() == int.class)
            u.getClass().getMethod("putInt", Object.class, long.class, int.class).invoke(u, o, off, (Integer) v);
        else
            u.getClass().getMethod("putObject", Object.class, long.class, Object.class).invoke(u, o, off, v);
    }

    static Method method(Class<?> c, String name, Class<?>... args) {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                Method m = k.getDeclaredMethod(name, args);
                m.setAccessible(true);
                return m;
            } catch (NoSuchMethodException e) {
            }
        }
        throw new RuntimeException(name);
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        PrintWriter out = new PrintWriter(new FileWriter(a[0]));

        // calcModifierDamage
        Object[][] sources = {
            {"generic", DamageSource.generic}, {"inFire", DamageSource.inFire}, {"onFire", DamageSource.onFire},
            {"lava", DamageSource.lava}, {"fall", DamageSource.fall}, {"outOfWorld", DamageSource.outOfWorld},
            {"magic", DamageSource.magic}, {"explosion", DamageSource.setExplosionSource(null)},
            {"arrow", new EntityDamageSourceIndirect("arrow", null, null).setProjectile()},
            {"fireball", DamageSource.causeFireballDamage(null, null)},
        };
        int[] prot = {0, 1, 2, 3, 4};
        for (int t : prot) {
            Enchantment e = Enchantment.enchantmentsList[t];
            for (int l = 1; l <= 6; l++)
                for (Object[] s : sources)
                    out.println("p " + t + " " + l + " " + s[0] + " " + e.calcModifierDamage(l, (DamageSource) s[1]));
        }
        // calcDamageByCreature
        EnumCreatureAttribute[] attrs = {EnumCreatureAttribute.UNDEFINED, EnumCreatureAttribute.UNDEAD, EnumCreatureAttribute.ARTHROPOD};
        for (int id = 16; id <= 18; id++)
            for (int l = 1; l <= 6; l++)
                for (int k = 0; k < 3; k++)
                    out.println("c " + id + " " + l + " " + k + " " + Enchantment.enchantmentsList[id].calcDamageByCreature(l, attrs[k]));
        // attemptDamageItem with Unbreaking
        int[] items = {278, 311, 261, 359};
        for (int it : items)
            for (int u = 0; u <= 3; u++)
                for (int n = 1; n <= 3; n++)
                    for (int seed = 0; seed < 12; seed++) {
                        ItemStack s = new ItemStack(Item.getItemById(it), 1, Item.getItemById(it).getMaxDamage() - 2);
                        if (u > 0) s.addEnchantment(Enchantment.unbreaking, u);
                        Random r = new Random(seed);
                        boolean broke = s.attemptDamageItem(n, r);
                        out.println("u " + it + " " + u + " " + n + " " + seed + " " + s.getItemDamage() + " " + broke + " |" + r.nextInt());
                    }
        // Thorns
        int[] tl = {1, 2, 3, 11, 13};
        for (int l : tl)
            for (int seed = 0; seed < 16; seed++) {
                Random r = new Random(seed);
                boolean hit = EnchantmentThorns.shouldHit(l, r);
                int d = EnchantmentThorns.getDamage(l, r);
                out.println("t " + l + " " + seed + " " + hit + " " + d + " |" + r.nextInt());
            }

        // mob loot
        Object u = unsafe();
        FakeWorld w = (FakeWorld) u.getClass().getMethod("allocateInstance", Class.class).invoke(u, FakeWorld.class);
        put(w, FakeWorld.class, "log", new ArrayList<String>());
        put(w, World.class, "isRemote", false);
        put(w, World.class, "provider", WorldProvider.getProviderForDimension(0));
        put(w, World.class, "worldInfo", new WorldInfo(new NBTTagCompound()));
        put(w, World.class, "rand", new Random(1));
        put(w, World.class, "theCalendar", java.util.Calendar.getInstance());
        Object[][] mobs = {
            {"Pig", EntityPig.class, 0}, {"Cow", EntityCow.class, 0}, {"MushroomCow", EntityMooshroom.class, 0},
            {"Sheep", EntitySheep.class, 5}, {"Sheep", EntitySheep.class, -1}, {"Chicken", EntityChicken.class, 0},
            {"Zombie", EntityZombie.class, 0}, {"Skeleton", EntitySkeleton.class, 0}, {"Skeleton", EntitySkeleton.class, 1},
            {"Creeper", EntityCreeper.class, 0}, {"Spider", EntitySpider.class, 0}, {"CaveSpider", EntityCaveSpider.class, 0},
            {"Witch", EntityWitch.class, 0}, {"Slime", EntitySlime.class, 1}, {"Slime", EntitySlime.class, 2},
            {"LavaSlime", EntityMagmaCube.class, 1}, {"LavaSlime", EntityMagmaCube.class, 2}, {"LavaSlime", EntityMagmaCube.class, 4},
            {"Enderman", EntityEnderman.class, 0}, {"Squid", EntitySquid.class, 0},
            {"EntityHorse", EntityHorse.class, 0}, {"EntityHorse", EntityHorse.class, 1}, {"EntityHorse", EntityHorse.class, 3},
            {"EntityHorse", EntityHorse.class, 4}, {"VillagerGolem", EntityIronGolem.class, 0}, {"Ghast", EntityGhast.class, 0},
            {"PigZombie", EntityPigZombie.class, 0}, {"Blaze", EntityBlaze.class, 0}, {"Villager", EntityVillager.class, 0},
            {"Silverfish", EntitySilverfish.class, 0}, {"Bat", EntityBat.class, 0}, {"Wolf", EntityWolf.class, 0},
            {"Ozelot", EntityOcelot.class, 0},
        };
        Method few = method(EntityLivingBase.class, "dropFewItems", boolean.class, int.class);
        Method rare = method(EntityLivingBase.class, "dropRareDrop", int.class);
        for (Object[] m : mobs) {
            Class<?> c = (Class<?>) m[1];
            int variant = (Integer) m[2];
            for (int burning = 0; burning < 2; burning++)
                for (int hit = 0; hit < 2; hit++)
                    for (int looting = 0; looting <= 3; looting++)
                        for (int seed = 0; seed < 6; seed++) {
                            EntityLiving e = (EntityLiving) c.getConstructor(World.class).newInstance(w);
                            if (e instanceof EntitySkeleton) ((EntitySkeleton) e).setSkeletonType(variant);
                            if (e instanceof EntitySlime) method(EntitySlime.class, "setSlimeSize", int.class).invoke(e, variant);
                            if (e instanceof EntitySheep) {
                                if (variant < 0) ((EntitySheep) e).setSheared(true);
                                else ((EntitySheep) e).setFleeceColor(variant);
                            }
                            if (e instanceof EntityHorse) ((EntityHorse) e).setHorseType(variant);
                            put(e, Entity.class, "fire", burning * 100);
                            Random r = new Random(seed);
                            put(e, Entity.class, "rand", r);
                            w.log.clear();
                            String res;
                            try {
                                few.invoke(e, hit == 1, looting);
                                if (hit == 1) {
                                    int j = r.nextInt(200) - looting;
                                    if (j < 5) rare.invoke(e, j <= 0 ? 1 : 0);
                                }
                                res = String.join(" ", w.log) + " |" + r.nextInt();
                            } catch (Throwable t) {
                                res = "ERR " + t;
                            }
                            out.println("d " + m[0] + " " + variant + " " + burning + " " + hit + " " + looting + " " + seed + " " + res);
                        }
        }

        // onSpawnWithEgg(null) with a fixed local difficulty:
        //   s <mob> <tension> <hard> <seed> <slot0..4> loot=<0|1> |<mob next int> <world next int>
        // a slot is "-" or id:meta followed by .enchId/lvl for each enchantment
        Object[][] armed = {
            {"Skeleton", EntitySkeleton.class}, {"Zombie", EntityZombie.class}, {"PigZombie", EntityPigZombie.class},
        };
        float[] tensions = {0.0F, 0.25F, 0.6F, 1.0F, 1.5F};
        for (Object[] m : armed) {
            Class<?> c = (Class<?>) m[1];
            for (float t : tensions)
                for (int hard = 0; hard < 2; hard++)
                    for (int seed = 0; seed < 40; seed++) {
                        w.tension = t;
                        w.difficultySetting = hard == 1 ? net.minecraft.world.EnumDifficulty.HARD : net.minecraft.world.EnumDifficulty.NORMAL;
                        EntityLiving e = (EntityLiving) c.getConstructor(World.class).newInstance(w);
                        Random r = new Random(seed);
                        put(e, Entity.class, "rand", r);
                        w.rand = new Random(seed + 1000L);
                        String res;
                        try {
                            e.onSpawnWithEgg(null);
                            StringBuilder b = new StringBuilder();
                            for (int k = 0; k < 5; k++) {
                                ItemStack s = e.getEquipmentInSlot(k);
                                if (s == null) {
                                    b.append("- ");
                                    continue;
                                }
                                b.append(Item.getIdFromItem(s.getItem())).append(':').append(s.getItemDamage());
                                net.minecraft.nbt.NBTTagList l = s.getEnchantmentTagList();
                                if (l != null)
                                    for (int i = 0; i < l.tagCount(); i++)
                                        b.append('.').append(l.getCompoundTagAt(i).getShort("id")).append('/').append(l.getCompoundTagAt(i).getShort("lvl"));
                                b.append(' ');
                            }
                            b.append("loot=").append(e.canPickUpLoot() ? 1 : 0);
                            res = b + " |" + r.nextInt() + " " + w.rand.nextInt();
                        } catch (Throwable x) {
                            res = "ERR " + x;
                        }
                        out.println("s " + m[0] + " " + t + " " + hard + " " + seed + " " + res);
                    }
        }
        out.close();
    }
}
