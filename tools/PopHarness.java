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
 * Reference dump of populated chunks: loads a (2R+1)^2 square of chunks in a
 * fixed order through the real ChunkProviderServer (structures off, animal
 * spawning off) and writes blocks, metadata, sky light and block light of the
 * populated ones.
 *
 * Usage: PopHarness <seed> <centreChunkX> <centreChunkZ> <radius> <out>
 */
public class PopHarness {
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
        long seed = Long.parseLong(a[0]);
        int ox = Integer.parseInt(a[1]), oz = Integer.parseInt(a[2]), r = Integer.parseInt(a[3]);
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
        WorldProvider p = WorldProvider.getProviderForDimension(0);
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
        ChunkProviderGenerate gen = new ChunkProviderGenerate(w, seed, features);
        ChunkProviderServer server = new ChunkProviderServer(w, null, gen);
        set(World.class, w, "chunkProvider", server);
        set(WorldServer.class, w, "theChunkProviderServer", server);

        for (int cx = -r; cx <= r; cx++)
            for (int cz = -r; cz <= r; cz++)
            {
                if (System.getenv("TRACE_PRECHECK") != null && cx == r && cz == r) {
                    String[] q = System.getenv("TRACE_CHUNK").split(",");
                    Chunk c = server.provideChunk(Integer.parseInt(q[0]), Integer.parseInt(q[1]));
                    c.checkLight();
                    long sum = 0;
                    for (int x = 0; x < 16; x++) for (int z = 0; z < 16; z++) for (int y = 80; y < 96; y++) sum += c.getSavedLightValue(EnumSkyBlock.Sky, x, y, z);
                    System.out.println("step precheck sum5=" + sum + " lightpop=" + c.isLightPopulated);
                    c.isTerrainPopulated = false;
                    if (System.getenv("TRACE_LAKE") != null) {
                        int pcx = Integer.parseInt(q[0]), pcz = Integer.parseInt(q[1]);
                        Random rr = new Random(seed);
                        long a1 = rr.nextLong() / 2L * 2L + 1L, b1 = rr.nextLong() / 2L * 2L + 1L;
                        rr.setSeed((long) pcx * a1 + (long) pcz * b1 ^ seed);
                        if (rr.nextInt(4) == 0) {
                            int lx = pcx * 16 + rr.nextInt(16) + 8, ly = rr.nextInt(256), lz = pcz * 16 + rr.nextInt(16) + 8;
                            System.out.println("   water lake at " + lx + "," + ly + "," + lz);
                            boolean ok = new net.minecraft.world.gen.feature.WorldGenLakes(net.minecraft.init.Blocks.water).generate(w, rr, lx, ly, lz);
                            sum = 0;
                            for (int x = 0; x < 16; x++) for (int z = 0; z < 16; z++) for (int y = 80; y < 96; y++) sum += c.getSavedLightValue(EnumSkyBlock.Sky, x, y, z);
                            System.out.println("   water lake: sum5=" + sum + " ok=" + ok);
                        }
                        System.exit(0);
                    }
                }
                server.loadChunk(ox + cx, oz + cz);
                String tc = System.getenv("TRACE_CHUNK");
                if (tc != null) {
                    String[] q = tc.split(",");
                    int tx = Integer.parseInt(q[0]), tz = Integer.parseInt(q[1]);
                    if (server.chunkExists(tx, tz)) {
                        Chunk c = server.provideChunk(tx, tz);
                        int mask = 0; long sum = 0;
                        for (int i = 0; i < 16; i++) if (c.getBlockStorageArray()[i] != null) mask |= 1 << i;
                        for (int x = 0; x < 16; x++) for (int z = 0; z < 16; z++) for (int y = 80; y < 96; y++) sum += c.getSavedLightValue(EnumSkyBlock.Sky, x, y, z);
                        System.out.println("step " + (ox + cx) + "," + (oz + cz) + " mask=" + mask + " sum5=" + sum + " pop=" + c.isTerrainPopulated);
                    }
                }
            }

        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(new FileOutputStream(a[4])))) {
            byte[] blocks = new byte[65536], meta = new byte[65536], sky = new byte[65536], blk = new byte[65536];
            for (int cx = -r; cx < r; cx++)
                for (int cz = -r; cz < r; cz++) {
                    Chunk c = server.provideChunk(ox + cx, oz + cz);
                    if (!c.isTerrainPopulated) throw new IllegalStateException("not populated: " + cx + "," + cz);
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
        System.out.println("done, chunks loaded: " + server.getLoadedChunkCount());
    }
}
