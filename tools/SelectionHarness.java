import java.io.*;
import net.minecraft.block.Block;
import net.minecraft.block.BlockCactus;
import net.minecraft.block.BlockCake;
import net.minecraft.block.BlockStairs;
import net.minecraft.init.Blocks;
import net.minecraft.init.Bootstrap;
import net.minecraft.tileentity.TileEntity;
import net.minecraft.world.IBlockAccess;
import net.minecraft.world.biome.BiomeGenBase;

/**
 * Reference selection boxes: every block id and metadata alone at (0, 64, 0)
 * with air around it, as RenderGlobal.drawSelectionBox sees it
 * (setBlockBoundsBasedOnState, then getSelectedBoundingBoxFromPool with
 * the overrides of BlockCactus, BlockCake and BlockStairs).
 * Output: tools/selection_1710.txt, "id meta minX minY minZ maxX maxY maxZ".
 */
public class SelectionHarness {
    static Block block;
    static int meta;

    static class Stub implements IBlockAccess {
        public Block getBlock(int x, int y, int z) {
            return x == 0 && y == 64 && z == 0 ? block : Blocks.air;
        }
        public TileEntity getTileEntity(int x, int y, int z) { return null; }
        public int getLightBrightnessForSkyBlocks(int x, int y, int z, int l) { return 0; }
        public int getBlockMetadata(int x, int y, int z) {
            return x == 0 && y == 64 && z == 0 ? meta : 0;
        }
        public boolean isAirBlock(int x, int y, int z) {
            return getBlock(x, y, z).getMaterial() == net.minecraft.block.material.Material.air;
        }
        public BiomeGenBase getBiomeGenForCoords(int x, int z) { return BiomeGenBase.plains; }
        public int getHeight() { return 256; }
        public boolean extendedLevelsInChunkCache() { return false; }
        public int isBlockProvidingPowerTo(int x, int y, int z, int s) { return 0; }
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        Stub w = new Stub();
        try (PrintWriter out = new PrintWriter(new FileWriter(a[0]))) {
            for (int id = 1; id < 256; id++) {
                block = Block.getBlockById(id);
                if (block == null || block == Blocks.air) continue;
                for (meta = 0; meta < 16; meta++) {
                    double[] b;
                    if (block instanceof BlockCactus) {
                        b = new double[] {0.0625, 0, 0.0625, 0.9375, 1, 0.9375};
                    } else if (block instanceof BlockCake) {
                        b = new double[] {(1 + meta * 2) / 16.0F, 0, 0.0625F, 1 - 0.0625F, 0.5F, 1 - 0.0625F};
                    } else if (block instanceof BlockStairs) {
                        b = new double[] {0, 0, 0, 1, 1, 1};
                    } else {
                        try {
                            block.setBlockBoundsBasedOnState(w, 0, 64, 0);
                        } catch (Throwable t) {
                            continue;
                        }
                        b = new double[] {block.getBlockBoundsMinX(), block.getBlockBoundsMinY(), block.getBlockBoundsMinZ(),
                                block.getBlockBoundsMaxX(), block.getBlockBoundsMaxY(), block.getBlockBoundsMaxZ()};
                    }
                    StringBuilder sb = new StringBuilder();
                    sb.append(id).append(' ').append(meta);
                    for (double v : b) sb.append(' ').append((float) v);
                    out.println(sb);
                }
            }
        }
    }
}
