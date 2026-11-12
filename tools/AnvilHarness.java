import java.io.*;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.*;
import net.minecraft.block.Block;
import net.minecraft.init.Bootstrap;
import net.minecraft.profiler.Profiler;
import net.minecraft.util.IntHashMap;
import net.minecraft.world.*;
import net.minecraft.world.biome.BiomeGenBase;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.gen.ChunkProviderGenerate;
import net.minecraft.world.gen.ChunkProviderServer;
import net.minecraft.world.storage.WorldInfo;

/**
 * Anvil cross-check. "write <seed> <cx> <cz> <r> <dir>": generates and
 * populates chunks with the real code and saves them with AnvilChunkLoader's
 * own NBT writer into region files. "read <seed> <cx> <cz> <r> <dir> <out>":
 * loads region chunks (written by either side) with AnvilChunkLoader and dumps
 * them in the PopHarness format.
 *
 * Original description of the setup: reference dump of populated chunks: loads a (2R+1)^2 square of chunks in a
 * fixed order through the real ChunkProviderServer (structures off, animal
 * spawning off) and writes blocks, metadata, sky light and block light of the
 * populated ones.
 *
 * Usage: PopHarness <seed> <centreChunkX> <centreChunkZ> <radius> <out>
 */
public class AnvilHarness {
    static void set(Class<?> c, Object o, String name, Object v) throws Exception {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        f.set(o, v);
    }

    /** Gives every collection-like field the value its initialiser would have. */
    static void initFields(Class<?> c, Object o) throws Exception {
        for (Field f : c.getDeclaredFields()) {
            if (Modifier.isStatic(f.getModifiers())) continue;
            f.setAccessible(true);
            if (f.get(o) != null && !f.getType().isPrimitive()) continue;
            Class<?> t = f.getType();
            if (t == TreeSet.class) f.set(o, new TreeSet());
            else if (t == Set.class) f.set(o, new HashSet());
            else if (t == List.class || t == ArrayList.class) f.set(o, new ArrayList());
            else if (t == int[].class) f.set(o, new int[32768]);
            else if (t == Random.class) f.set(o, new Random(1L));
            else if (t == Profiler.class) f.set(o, new Profiler());
            else if (t == IntHashMap.class) f.set(o, new IntHashMap());
        }
    }

    public static void main(String[] a) throws Exception {
        String mode = a[0];
        a = java.util.Arrays.copyOfRange(a, 1, a.length);
        long seed = Long.parseLong(a[0]);
        int ox = Integer.parseInt(a[1]), oz = Integer.parseInt(a[2]), r = Integer.parseInt(a[3]);
        File dir = new File(a[4]);
        Bootstrap.register();
        Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        uf.setAccessible(true);
        sun.misc.Unsafe u = (sun.misc.Unsafe) uf.get(null);
        WorldServer w = (WorldServer) u.allocateInstance(WorldServer.class);
        initFields(World.class, w);
        initFields(WorldServer.class, w);

        Class<?> gt = Class.forName("net.minecraft.world.WorldSettings$GameType");
        Object survival = null;
        for (Object o : gt.getEnumConstants()) if (((Enum<?>) o).name().equals("SURVIVAL")) survival = o;
        WorldSettings ws = (WorldSettings) WorldSettings.class.getConstructor(long.class, gt, boolean.class, boolean.class, WorldType.class)
            .newInstance(seed, survival, false, false, WorldType.DEFAULT);
        WorldInfo info = new WorldInfo(ws, "x");
        set(World.class, w, "worldInfo", info);
        info.getGameRulesInstance().setOrCreateGameRule("doTileDrops", "false");
        int dim = System.getenv("DIM") == null ? 0 : Integer.parseInt(System.getenv("DIM"));
        WorldProvider p = WorldProvider.getProviderForDimension(dim);
        set(World.class, w, "provider", p);
        p.registerWorld(w);

        // No animals: SpawnerAnimals.performWorldGenSpawning needs a full server.
        for (BiomeGenBase b : BiomeGenBase.getBiomeGenArray()) {
            if (b == null) continue;
            for (Field f : BiomeGenBase.class.getDeclaredFields()) {
                if (f.getType() == List.class && !Modifier.isStatic(f.getModifiers())) {
                    f.setAccessible(true);
                    ((List) f.get(b)).clear();
                }
            }
        }

        boolean features = System.getenv("NO_FEATURES") == null;
        set(World.class, w, "mapStorage", new net.minecraft.world.storage.MapStorage(null));
        net.minecraft.world.chunk.IChunkProvider gen = dim == -1
            ? new net.minecraft.world.gen.ChunkProviderHell(w, seed)
            : new ChunkProviderGenerate(w, seed, features);
        ChunkProviderServer server = new ChunkProviderServer(w, null, gen);
        set(World.class, w, "chunkProvider", server);
        set(WorldServer.class, w, "theChunkProviderServer", server);

        net.minecraft.world.chunk.storage.AnvilChunkLoader loader = new net.minecraft.world.chunk.storage.AnvilChunkLoader(dir);
        java.lang.reflect.Method write = net.minecraft.world.chunk.storage.AnvilChunkLoader.class.getDeclaredMethod(
            "writeChunkToNBT", Chunk.class, World.class, net.minecraft.nbt.NBTTagCompound.class);
        write.setAccessible(true);
        java.lang.reflect.Method read = net.minecraft.world.chunk.storage.AnvilChunkLoader.class.getDeclaredMethod(
            "readChunkFromNBT", World.class, net.minecraft.nbt.NBTTagCompound.class);
        read.setAccessible(true);
        if (mode.equals("write")) {
            for (int cx = -r; cx <= r; cx++)
                for (int cz = -r; cz <= r; cz++)
                    server.loadChunk(ox + cx, oz + cz);
            for (int cx = -r; cx < r; cx++)
                for (int cz = -r; cz < r; cz++) {
                    Chunk c = server.provideChunk(ox + cx, oz + cz);
                    net.minecraft.nbt.NBTTagCompound root = new net.minecraft.nbt.NBTTagCompound();
                    net.minecraft.nbt.NBTTagCompound level = new net.minecraft.nbt.NBTTagCompound();
                    root.setTag("Level", level);
                    write.invoke(loader, c, w, level);
                    DataOutputStream out = net.minecraft.world.chunk.storage.RegionFileCache.getChunkOutputStream(dir, ox + cx, oz + cz);
                    net.minecraft.nbt.CompressedStreamTools.write(root, out);
                    out.close();
                }
            net.minecraft.world.chunk.storage.RegionFileCache.clearRegionFileReferences();
            System.out.println("written");
            return;
        }
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(a[5])))) {
            for (int cx = -r; cx < r; cx++)
                for (int cz = -r; cz < r; cz++) {
                    DataInputStream in = net.minecraft.world.chunk.storage.RegionFileCache.getChunkInputStream(dir, ox + cx, oz + cz);
                    net.minecraft.nbt.NBTTagCompound root = net.minecraft.nbt.CompressedStreamTools.read(in);
                    Chunk c = (Chunk) read.invoke(loader, w, root.getCompoundTag("Level"));
                    for (int x = 0; x < 16; x++)
                        for (int z = 0; z < 16; z++)
                            for (int y = 0; y < 256; y++) {
                            }
                    byte[] blocks = new byte[65536], meta = new byte[65536], sky = new byte[65536], blk = new byte[65536];
                    for (int x = 0; x < 16; x++)
                        for (int z = 0; z < 16; z++)
                            for (int y = 0; y < 256; y++) {
                                int i = (x * 16 + z) * 256 + y;
                                blocks[i] = (byte) Block.getIdFromBlock(c.getBlock(x, y, z));
                                meta[i] = (byte) c.getBlockMetadata(x, y, z);
                                sky[i] = (byte) c.getSavedLightValue(EnumSkyBlock.Sky, x, y, z);
                                blk[i] = (byte) c.getSavedLightValue(EnumSkyBlock.Block, x, y, z);
                            }
                    out.write(blocks);
                    out.write(meta);
                    out.write(sky);
                    out.write(blk);
                    int mask = 0;
                    for (int i = 0; i < 16; i++) if (c.getBlockStorageArray()[i] != null) mask |= 1 << i;
                    out.writeShort(mask);
                    for (int z = 0; z < 16; z++) for (int x = 0; x < 16; x++) out.writeShort(c.getHeightValue(x, z));
                }
        }
        System.out.println("read");
    }
}
