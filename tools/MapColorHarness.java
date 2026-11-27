import java.io.*;
import net.minecraft.block.Block;
import net.minecraft.block.material.MapColor;
import net.minecraft.init.Bootstrap;

/**
 * Reference map colours: Block.getMapColor(meta).colorIndex for every block
 * id and metadata, and MapColor.colorValue for every index.
 * Output: tools/mapcolor_1710.txt, one line of 16 indices per block id
 * (256 lines), then one line of the 64 colour values.
 */
public class MapColorHarness {
    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        PrintWriter out = new PrintWriter(new FileWriter("tools/mapcolor_1710.txt"));
        for (int id = 0; id < 256; id++) {
            Block b = Block.getBlockById(id);
            StringBuilder sb = new StringBuilder();
            for (int m = 0; m < 16; m++) {
                int c = 0;
                try {
                    MapColor mc = b.getMapColor(m);
                    c = mc == null ? 0 : mc.colorIndex;
                } catch (Throwable t) {
                    c = 0;
                }
                if (m > 0) sb.append(' ');
                sb.append(c);
            }
            out.println(sb);
        }
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 64; i++) {
            if (i > 0) sb.append(' ');
            MapColor mc = MapColor.mapColorArray[i];
            sb.append(mc == null ? -1 : mc.colorValue);
        }
        out.println(sb);
        out.close();
    }
}
