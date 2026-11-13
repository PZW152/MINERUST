import java.io.*;
import net.minecraft.block.Block;
import net.minecraft.client.renderer.texture.IIconRegister;
import net.minecraft.init.Bootstrap;
import net.minecraft.util.IIcon;

/**
 * Reference block textures: for every block id, metadata and side the
 * name of the icon Block.getIcon(side, meta) returns after
 * registerBlockIcons, one line per (id, meta):
 *   id meta bottom top north south west east
 * "-" where the block has no icon for that side (or throws).
 */
public class IconHarness {
    static class Icon implements IIcon {
        final String name;
        Icon(String n) { name = n; }
        public int getIconWidth() { return 16; }
        public int getIconHeight() { return 16; }
        public float getMinU() { return 0; }
        public float getMaxU() { return 1; }
        public float getInterpolatedU(double d) { return (float) d / 16; }
        public float getMinV() { return 0; }
        public float getMaxV() { return 1; }
        public float getInterpolatedV(double d) { return (float) d / 16; }
        public String getIconName() { return name; }
    }

    public static void main(String[] a) throws Exception {
        Bootstrap.register();
        IIconRegister reg = new IIconRegister() {
            public IIcon registerIcon(String n) { return new Icon(n); }
        };
        PrintWriter w = new PrintWriter(new FileWriter(a[0]));
        // every block first: some take their icons from other blocks
        for (int id = 1; id < 256; id++) {
            Block b = Block.getBlockById(id);
            if (b == null || Block.getIdFromBlock(b) != id) continue;
            try {
                b.registerBlockIcons(reg);
            } catch (Throwable t) {
                w.println("# " + id + " registerBlockIcons: " + t);
            }
        }
        for (int id = 1; id < 256; id++) {
            Block b = Block.getBlockById(id);
            if (b == null || Block.getIdFromBlock(b) != id) continue;
            for (int meta = 0; meta < 16; meta++) {
                StringBuilder s = new StringBuilder(id + " " + meta);
                for (int side = 0; side < 6; side++) {
                    String n = "-";
                    try {
                        IIcon i = b.getIcon(side, meta);
                        if (i != null) n = i.getIconName();
                    } catch (Throwable t) {
                        n = "-";
                    }
                    s.append(' ').append(n);
                }
                w.println(s);
            }
        }
        w.close();
    }
}
