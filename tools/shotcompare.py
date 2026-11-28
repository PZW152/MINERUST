"""Screenshot comparison between the real Minecraft 1.7.10 client and MINERUST.

Both games render the same scenes (seed, world time with the daylight cycle
stopped, flying creative player at a fixed position and view direction, window
size, identical options.txt). For each scene the tool writes
    <out>/<scene>/vanilla.png, minerust.png, diff.png (heat map), side.png
and a summary table <out>/report.md.

Exact equality is not expected: vanilla draws with fixed-function OpenGL,
MINERUST with Vulkan shaders, and mobs, clouds and particles move between the
two runs. The numbers are for spotting regressions and large differences;
look at side.png / diff.png to judge them.

Usage (Windows, from the repository root):
    python tools/shotcompare.py [--exe target/release/minerust.exe] [--out shots] [scene ...]

Environment (defaults match HANDOFF section 3):
    MC_ROOT   launcher root holding versions/1.7.10, libraries, assets (%APPDATA%/Sapling)
    MCP_JAR   the MCP-remapped jar (reference decompile)
    JAVA8     a Java 8 java.exe for the vanilla client (1.7.10 + LWJGL 2 need Java 8)
    JAVAC     javac that can target Java 8 (any JDK >= 9, used with --release 8)
"""
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
APPDATA = Path(os.environ.get("APPDATA", ""))
MC_ROOT = Path(os.environ.get("MC_ROOT", APPDATA / "Sapling"))
MCP_JAR = Path(os.environ.get("MCP_JAR", r"D:\Claude\mc1710-ref\mc-1.7.10-reverse\deobfuscated\Minecraft-1.7.10-mcp.jar"))
JAVA8 = os.environ.get("JAVA8", str(APPDATA / ".minecraft/runtime/jre-legacy/bin/java.exe"))
JAVAC = os.environ.get("JAVAC", shutil.which("javac") or str(Path(os.environ.get("LOCALAPPDATA", "")) / "Programs/jdk-21/bin/javac.exe"))

WIDTH, HEIGHT = 854, 480
SETTLE = 4.0  # seconds after the world has finished building, as in HANDOFF 5.1

# The same options.txt for both games (GameSettings keys).
OPTIONS = {
    "renderDistance": "8",
    "fancyGraphics": "true",
    "ao": "2",
    "clouds": "true",
    "guiScale": "0",
    "particles": "0",
    "bobView": "true",
    "fov": "0.0",
    "gamma": "0.0",
    "mipmapLevels": "4",
    "anisotropicFiltering": "1",
    "maxFps": "120",
    "fboEnable": "true",
    "lang": "en_US",
    "difficulty": "2",
    "pauseOnLostFocus": "false",  # the windows run in the background
}

# name: seed, time, x y z, yaw pitch, extra flags. Seed 12345 spawns at -242.5 84 130.5.
SCENES = {
    "day":     dict(seed=12345, time=6000,  pos=(-242.5, 90.0, 130.5),  look=(0.0, 15.0)),
    "day_hud": dict(seed=12345, time=6000,  pos=(-242.5, 90.0, 130.5),  look=(0.0, 15.0), hud=True),
    "sunrise": dict(seed=12345, time=23500, pos=(-242.5, 90.0, 130.5),  look=(-90.0, 0.0)),
    "sunset":  dict(seed=12345, time=12500, pos=(-242.5, 90.0, 130.5),  look=(90.0, 0.0)),
    "night":   dict(seed=12345, time=18000, pos=(-242.5, 90.0, 130.5),  look=(0.0, -40.0)),
    "above":   dict(seed=12345, time=6000,  pos=(-242.5, 130.0, 130.5), look=(45.0, 60.0)),
    "f3":      dict(seed=12345, time=6000,  pos=(-242.5, 90.0, 130.5),  look=(-90.0, 60.0), hud=True, debug=True),
    # RenderBlocks types built on a floating stone platform: torch, repeaters (every
    # facing and delay, one locked by a powered repeater), ladder, rails, crops.
    "redstone": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 55.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 133 50 5", "-246 96 134 76 5",
        "-245 96 133 93 0", "-244 96 133 93 5", "-243 96 133 93 10", "-242 96 133 93 15",
        "-245 96 135 152", "-244 96 135 94 1", "-243 96 135 93 0",
        "-240 96 133 1", "-240 96 134 65 3",
        "-242 96 137 66 0", "-241 96 137 66 6", "-240 96 137 27 1", "-239 96 137 27 9",
        "-245 95 137 60 7", "-245 96 137 59 7", "-244 95 137 60 7", "-244 96 137 141 3",
        "-246 95 137 88", "-246 96 137 115 3",
    ])),
    # render type 0 blocks with bounds (smooth lighting), slabs, snow layers, cactus.
    "shapes": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 55.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 133 44 0", "-245 96 133 44 8", "-244 96 133 126 0", "-243 96 133 78 0",
        "-242 96 133 78 3", "-241 96 133 78 6", "-240 96 133 171 14", "-239 96 133 70 0",
        "-246 96 135 12", "-246 97 135 81", "-246 98 135 81",
        "-244 95 135 60 7", "-243 96 135 116",
        "-241 96 135 1", "-242 96 135 96 2",
        "-239 96 135 1", "-239 97 135 1", "-240 96 135 77 2", "-240 97 135 143 2",
        "-243 96 137 72 0", "-241 96 137 126 8", "-240 96 137 44 1", "-239 96 137 44 9",
    ])),
    # renderBlockStairs (straight, upside down, inner and outer corners) and renderBlockFence.
    "stairs": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 55.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 133 53 0", "-245 96 133 53 1", "-244 96 133 53 4", "-243 96 133 108 2", "-242 96 133 109 3",
        "-240 96 133 53 0", "-240 96 134 53 2", "-239 96 133 134 3", "-238 96 133 134 0",
        "-246 96 136 85", "-245 96 136 85", "-244 96 136 85", "-244 96 137 85",
        "-242 96 136 85", "-240 96 136 113", "-240 96 137 113", "-239 96 137 1", "-238 96 137 128 6",
    ])),
    # stained glass panes (colours, joining stained glass, glass panes and iron
    # bars) and the weighted pressure plates.
    "panes": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 50.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 134 160 0", "-245 96 134 160 14", "-244 96 134 160 11", "-243 96 134 160 5",
        "-242 96 134 95 4", "-241 96 134 160 4", "-240 96 134 102", "-239 96 134 101",
        "-246 96 135 160 10", "-246 96 136 160 10",
        "-244 96 137 147", "-242 96 137 148", "-240 96 137 160 15",
    ])),
    # hoppers in every facing (renderBlockHopper), a dropper and a dispenser.
    "hopper": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 50.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 134 154 0", "-244 96 134 154 2", "-242 96 134 154 3", "-240 96 134 154 4",
        "-238 96 134 154 5", "-245 96 137 158 2", "-242 96 137 23 2", "-239 96 137 158 1",
    ])),
    # renderBlockRedstoneComparator: compare and subtract mode, each facing, and
    # two lit by the cake behind them (placed after the comparators).
    "comparator": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 55.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-247, -237) for z in range(132, 139)] + [
        "-246 96 134 149 0", "-244 96 134 149 1", "-242 96 134 149 2", "-240 96 134 149 3",
        "-246 96 136 149 4", "-244 96 136 149 5",
        "-241 96 137 149 0", "-241 96 138 92 0", "-239 96 137 149 4", "-239 96 138 92 0",
        "-238 96 135 151", "-246 96 138 151",
    ])),
    # Snow layers at night with a one-block trench, looked down on with the
    # selection box (the user's report: dark sides and the outline).
    "snowtrench": dict(seed=12345, time=18000, pos=(-241.5, 97.0, 129.5), look=(0.0, 45.0), hud=True, setblock=";".join(
        [f"{x} 95 {z} 2" for x in range(-249, -235) for z in range(126, 142)] +
        [f"{x} 96 {z} 78" for x in range(-249, -235) for z in range(126, 142) if not (x == -243 and z < 134)] +
        [f"{x} {y} {z} 0" for x in range(-249, -235) for y in range(97, 101) for z in range(126, 142)]
    )),
    # TileEntityChestRenderer for plain and trapped chests (single and double)
    # and TileEntityEnderChestRenderer, in several facings.
    "chests": dict(seed=12345, time=6000, pos=(-242.5, 98.5, 131.5), look=(0.0, 45.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-248, -236) for z in range(132, 140)] + [
        "-246 96 134 54 3", "-244 96 134 146 3", "-242 96 134 130 3", "-240 96 134 130 4",
        "-246 96 137 146 2", "-245 96 137 146 2", "-242 96 137 54 5", "-242 96 138 54 5",
        "-239 96 137 146 3", "-239 96 138 146 3",
    ])),
    # BlockPumpkin.onBlockAdded: a snow golem and an iron golem built from
    # blocks (RenderSnowMan with its pumpkin, RenderIronGolem); they move.
    "golems": dict(seed=12345, time=6000, pos=(-242.5, 97.0, 131.5), look=(0.0, 15.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-250, -234) for z in range(131, 142)] +
        [f"{x} {y} {z} 20" for x in (-251, -234) for y in (96, 97) for z in range(131, 142)] +
        [f"{x} {y} {z} 20" for x in range(-250, -234) for y in (96, 97) for z in (130, 142)] + [
        "-245 96 136 80", "-245 97 136 80", "-245 98 136 86 0",
        "-239 96 136 42", "-239 97 136 42", "-240 97 136 42", "-238 97 136 42", "-239 98 136 91 0",
    ])),
    # RenderBlocks.renderBlockCocoa: pods of each stage on the four sides of jungle logs
    "cocoa": dict(seed=12345, time=6000, pos=(-242.5, 97.0, 131.5), look=(0.0, 20.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-249, -235) for z in range(131, 140)] +
        [f"{x} {y} 136 17 3" for x in (-246, -242, -238) for y in (96, 97, 98)] + [
        "-246 97 135 127 0", "-246 96 135 127 4", "-247 97 136 127 1", "-245 97 136 127 3",
        "-242 97 135 127 8", "-242 96 135 127 2", "-243 97 136 127 9", "-241 97 136 127 11",
        "-238 97 135 127 4", "-239 96 136 127 5", "-237 96 136 127 7", "-238 98 135 127 8",
    ])),
    # BlockDoor.getIcon: wooden and iron doors in each facing and hinge, closed and open
    "doors": dict(seed=12345, time=6000, pos=(-242.5, 97.0, 131.5), look=(0.0, 15.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-250, -234) for z in range(131, 141)] +
        [f"{-249 + 2 * i} 96 135 {64 if i < 4 else 71} {i % 4}" for i in range(8)] +
        [f"{-249 + 2 * i} 97 135 {64 if i < 4 else 71} {8 + (i // 2) % 2}" for i in range(8)] +
        [f"{-249 + 2 * i} 96 138 {64 if i < 4 else 71} {4 + i % 4}" for i in range(8)] +
        [f"{-249 + 2 * i} 97 138 {64 if i < 4 else 71} {8 + (i // 2) % 2}" for i in range(8)])),
    # RenderBlocks.renderBlockFlowerpot: a pot of each kind createNewTileEntity makes from metadata
    "pots": dict(seed=12345, time=6000, pos=(-241.5, 97.6, 132.0), look=(0.0, 25.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-249, -235) for z in range(131, 140)] +
        [f"{-248 + i} 96 135 140 {m}" for i, m in enumerate([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0])])),
    # renderBlockCauldron at each water level, end portal frames with and without an eye, a dragon egg
    "cauldrons": dict(seed=12345, time=6000, pos=(-241.5, 97.6, 131.5), look=(0.0, 30.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-249, -235) for z in range(131, 140)] +
        [f"{-246 + 2 * i} 96 135 118 {i}" for i in range(4)] +
        [f"{-246 + 2 * i} 96 137 120 {i * 2 + (i & 1) * 4}" for i in range(4)] + ["-238 96 135 122 0"])),
    # RenderPainting and RenderItemFrame on a wall: four paintings, framed
    # items as sprites and a block in each rotation, and an empty frame.
    "hanging": dict(seed=12345, time=6000, pos=(-242.5, 97.0, 131.5), look=(0.0, 5.0), setblock=";".join(
        [f"{x} 95 {z} 1" for x in range(-249, -235) for z in range(131, 138)] +
        [f"{x} {y} 137 1" for x in range(-249, -235) for y in range(96, 102)]), hang=";".join([
        "-247 98 137 2 P Kebab", "-245 98 137 2 P Pool", "-243 97 137 2 P Wanderer",
        "-240 98 137 2 P Fighters",
        "-247 96 137 2 F 264 0", "-245 96 137 2 F 1 1", "-243 96 137 2 F 276 2",
        "-241 96 137 2 F 35:14 3", "-238 97 137 2 F 0",
    ])),
}


def classpath():
    meta = json.loads((MC_ROOT / "versions/1.7.10/1.7.10.json").read_text(encoding="utf-8"))
    entries = [str(MCP_JAR)]  # MCP names first; the original jar after it only supplies resources
    for lib in meta["libraries"]:
        if "natives" in lib:
            continue
        group, name, version = lib["name"].split(":")
        jar = MC_ROOT / "libraries" / group.replace(".", "/") / name / version / f"{name}-{version}.jar"
        if not jar.exists():
            sys.exit(f"missing library: {jar}")
        entries.append(str(jar))
    entries.append(str(MC_ROOT / "versions/1.7.10/1.7.10.jar"))
    return ";".join(entries)


def write_options(folder: Path):
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "options.txt").write_text("".join(f"{k}:{v}\n" for k, v in OPTIONS.items()), encoding="utf-8")


def run_vanilla(cp: str, classes: Path, scene: dict, png: Path, game: Path):
    write_options(game)
    x, y, z = scene["pos"]
    yaw, pitch = scene["look"]
    flags = ([] if scene.get("hud") else ["--hide-gui"]) + (["--debug"] if scene.get("debug") else [])
    flags += ["--setblock", scene["setblock"]] if scene.get("setblock") else []
    flags += ["--hang", scene["hang"]] if scene.get("hang") else []
    cmd = [JAVA8, "-Xmx2G", f"-Djava.library.path={MC_ROOT / 'versions/1.7.10/natives'}",
           "-cp", f"{classes};{cp}", "ShotHarness",
           str(png), str(scene["seed"]), str(scene["time"]), str(x), str(y), str(z), str(yaw), str(pitch), str(SETTLE),
           *flags, "--",
           "--username", "Player", "--version", "1.7.10", "--gameDir", str(game),
           "--assetsDir", str(MC_ROOT / "assets"), "--assetIndex", "1.7.10",
           "--uuid", "00000000000000000000000000000000", "--accessToken", "0",
           "--userProperties", "{}", "--userType", "legacy",
           "--width", str(WIDTH), "--height", str(HEIGHT)]
    with open(game / "harness.log", "w", encoding="utf-8") as log:
        subprocess.run(cmd, cwd=game, stdout=log, stderr=subprocess.STDOUT, timeout=300)


def run_minerust(exe: Path, scene: dict, png: Path, game: Path):
    write_options(game)
    x, y, z = scene["pos"]
    yaw, pitch = scene["look"]
    flags = ([] if scene.get("hud") else ["--hide-gui"]) + (["--debug"] if scene.get("debug") else [])
    flags += ["--setblock", scene["setblock"]] if scene.get("setblock") else []
    flags += ["--hang", scene["hang"]] if scene.get("hang") else []
    cmd = [str(exe), "--seed", str(scene["seed"]), "--no-save", "--size", str(WIDTH), str(HEIGHT),
           "--time", str(scene["time"]), "--freeze-time", "--creative",
           "--pos", str(x), str(y), str(z), "--look", str(yaw), str(pitch), *flags,
           "--screenshot", str(png), str(SETTLE)]
    with open(game / "minerust.log", "w", encoding="utf-8") as log:
        subprocess.run(cmd, cwd=game, stdout=log, stderr=subprocess.STDOUT, timeout=300)


def compare(vanilla: Path, minerust: Path, folder: Path):
    a = Image.open(vanilla).convert("RGB")
    b = Image.open(minerust).convert("RGB")
    if a.size != b.size:
        return f"size {a.size} vs {b.size}"
    diff = ImageChops.difference(a, b)
    px = diff.get_flattened_data() if hasattr(diff, "get_flattened_data") else diff.getdata()
    worst = [max(p) for p in px]
    n = len(worst)
    mean = sum(sum(p) for p in px) / (3 * n)
    big = sum(1 for d in worst if d > 24) / n
    # heat map: black = equal, white = channel difference of 64 or more
    heat = Image.new("L", a.size)
    heat.putdata([min(255, d * 4) for d in worst])
    heat.save(folder / "diff.png")
    side = Image.new("RGB", (a.width * 3, a.height))
    side.paste(a, (0, 0))
    side.paste(b, (a.width, 0))
    side.paste(heat.convert("RGB"), (a.width * 2, 0))
    side.save(folder / "side.png")
    return f"mean channel diff {mean:.1f}, pixels off by >24: {100 * big:.1f}%"


def main():
    args = sys.argv[1:]
    exe = ROOT / "target/release/minerust.exe"
    out = ROOT / "shots"
    names = []
    while args:
        a = args.pop(0)
        if a == "--exe":
            exe = Path(args.pop(0)).resolve()
        elif a == "--out":
            out = Path(args.pop(0)).resolve()
        else:
            names.append(a)
    names = names or list(SCENES)
    out.mkdir(parents=True, exist_ok=True)

    classes = out / ".classes"
    classes.mkdir(exist_ok=True)
    cp = classpath()
    subprocess.run([JAVAC, "--release", "8", "-nowarn", "-Xlint:-options", "-cp", cp, "-d", str(classes),
                    str(ROOT / "tools/ShotHarness.java")], check=True)

    lines = ["| 場景 | 結果 |", "| --- | --- |"]
    for name in names:
        scene = SCENES[name]
        folder = out / name
        if folder.exists():
            shutil.rmtree(folder)
        folder.mkdir(parents=True)
        v, m = folder / "vanilla.png", folder / "minerust.png"
        print(f"{name}: vanilla ...", flush=True)
        run_vanilla(cp, classes, scene, v, folder / "vanilla-game")
        print(f"{name}: minerust ...", flush=True)
        run_minerust(exe, scene, m, folder / "minerust-game")
        if not v.exists() or not m.exists():
            result = "screenshot missing (" + ", ".join(p.name for p in (v, m) if not p.exists()) + "), see the logs"
        else:
            result = compare(v, m, folder)
        print(f"{name}: {result}", flush=True)
        lines.append(f"| `{name}` | {result} |")
    (out / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
