# Regenerates src/render/rb.rs from the decompiled RenderBlocks.java.
# Run from the repository root: sh tools/rb/gen.sh
set -e
D=$(dirname "$0")
J=${RENDER_BLOCKS:-/d/Claude/mc1710-ref/mc-1.7.10-reverse/src-java/net/minecraft/client/renderer/RenderBlocks.java}
AO="&mut self, mut var2: i32, mut var3: i32, mut var4: i32, var5: f32, var6: f32, var7: f32"
F="&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon"
python -I "$D/j2r.py" $J "$D/rb_gen.rs" \
 "renderStandardBlockWithAmbientOcclusion:std_ao:bool:$AO" \
 "renderStandardBlockWithAmbientOcclusionPartial:std_ao_partial:bool:$AO" \
 "renderStandardBlockWithColorMultiplier:std_flat:bool:$AO" \
 "renderFaceYNeg:face_y_neg::$F" "renderFaceYPos:face_y_pos::$F" "renderFaceZNeg:face_z_neg::$F" \
 "renderFaceZPos:face_z_pos::$F" "renderFaceXNeg:face_x_neg::$F" "renderFaceXPos:face_x_pos::$F"
python "$D/mkrb.py"
rm -f "$D/rb_gen.rs"
