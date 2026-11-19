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
    flags = [] if scene.get("hud") else ["--hide-gui"]
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
    flags = [] if scene.get("hud") else ["--hide-gui"]
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
