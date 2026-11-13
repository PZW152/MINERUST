#!/bin/sh
# Prints the Windows classpath the tools/*Harness.java programs run with.
#   MC_LIBS   launcher libraries folder (default: %APPDATA%/Sapling/libraries)
#   MCP_JAR   the MCP-remapped 1.7.10 jar (default: the reference decompile)
# Usage: CP=$(sh tools/classpath.sh); javac -cp "$CP" ...; java -cp "out;$CP" ...
LIBS=${MC_LIBS:-/c/Users/PZW152/AppData/Roaming/Sapling/libraries}
MCP=${MCP_JAR:-/d/Claude/mc1710-ref/mc-1.7.10-reverse/deobfuscated/Minecraft-1.7.10-mcp.jar}
cp="$(cygpath -w "$MCP")"
for j in com/google/guava/guava/17.0/guava-17.0.jar \
         com/google/code/gson/gson/2.2.4/gson-2.2.4.jar \
         org/apache/commons/commons-lang3/3.3.2/commons-lang3-3.3.2.jar \
         commons-io/commons-io/2.4/commons-io-2.4.jar \
         org/apache/logging/log4j/log4j-api/2.0-beta9/log4j-api-2.0-beta9.jar \
         org/apache/logging/log4j/log4j-core/2.0-beta9/log4j-core-2.0-beta9.jar \
         io/netty/netty-all/4.0.10.Final/netty-all-4.0.10.Final.jar \
         com/mojang/authlib/1.5.21/authlib-1.5.21.jar; do
  [ -e "$LIBS/$j" ] || echo "missing: $LIBS/$j" >&2
  cp="$cp;$(cygpath -w "$LIBS/$j")"
done
printf '%s' "$cp"
