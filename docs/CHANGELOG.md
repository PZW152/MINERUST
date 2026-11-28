# 變更紀錄

每次推送在最上面的日期下加一行：`提交編號` 與提交訊息的標題。細節寫在提交訊息、程式註解與
[PARITY_TODO.md](PARITY_TODO.md)（還沒做完的部分），不寫在這裡。提交前還不知道自己的編號，先只寫標題，
下一次推送時補上。

2026-11-20 以前的長篇開發日誌（每次的驗證方式、量測數字、找到的原因）保存在 git 歷史裡：
`git show c57b73a:docs/HANDOFF.md` 的第 9 節。

## 2026-11-28

- Chunks keep their InhabitedTime in Anvil saves (it was written as 0), so the regional difficulty that grows where the player spends time survives a reload. (1.7.10's level.dat has no Difficulty, which lives in options.txt; that to-do item was wrong and is gone)
- `8c82e71` - Jockeys survive a reload: a mob riding another is saved as one entry with its mount under `Riding` (Entity.writeToNBTOptional, writeMountToNBT) and mounted again on load (AnvilChunkLoader), and chickens keep IsChickenJockey
- `af89d1b` - Tab completes in chat (GuiChat.autocompletePlayerNames with MinecraftServer.getPossibleCompletions): command names, then each command's arguments (game rules and true/false, game modes, difficulties, time set/add and day/night, weather, item and block names for give and setblock, the player's name), the common prefix first, then cycling with the choices listed in one chat line that replaces itself (printChatMessageWithOptionalDeletion)
- `43baacd` - A hurt mob or player kicks its legs out (limbSwingAmount 1.5 in attackEntityFrom and handleHealthUpdate). The hurt camera tilt stays one way: the client's handleHealthUpdate sets attackedAtYaw to 0 in 1.7.10, so it never had a direction
- `110a28c` - Right-clicking a mob that does nothing with the held item uses it (Minecraft.func_147121_ag after interactWithEntitySendPacket is false): eating, drinking, throwing snowballs or eggs, drawing the bow while looking at an animal now work
- `f4fd2c7` - Puppies and kittens drawn as ModelWolf and ModelOcelot do with isChild: the wolf's head at full size moved down and forward, the kitten's at three quarters, the rest at half size
- `350e3a5` - shotcompare scene `sleep` (lying in a bed at night, `--sleep "x y z"` for MINERUST and ShotHarness): within 2.1 of the game
- `3322619` - Silverfish hide and call for help (EntitySilverfish.updateEntityActionState): an idle one with no path crawls into the stone, cobblestone or stone bricks beside it (the infested block of the matching kind), a hurt one wakes those hiding within 10 blocks a second later, nearest layers first and stopping at random, each one breaking out of its block (or leaving the plain block behind without mobGriefing); they prefer paths over stone
- `8226c4f` - Game rules (GameRules, CommandGameRule): `/gamerule` lists, reads and sets them, they are saved in level.dat (Data.GameRules) and in MINERUST saves, and they act where the game reads them: doDaylightCycle stops the clock on both sides, keepInventory keeps the items, the levels and the score through death, doMobSpawning stops natural spawning and portal pigmen, doMobLoot the mobs' drops and experience, doTileDrops the blocks' drops, doFireTick fire spread and lightning fires, naturalRegeneration healing from food, mobGriefing creeper, ghast, wither and wither skull block damage, endermen, the dragon, sheep eating grass, farmland trampled by mobs, door breaking and loot pickup. The server now sends the time every second (S03PacketTimeUpdate), so `/time set` and sleeping move the client's sky. On death the armour drops with the rest (dropAllItems) and the player drops experience (level × 7, at most 100) as the game does; sheep eating grass shows its breaking particles
- `0828086` - Zombie reinforcements on Hard (EntityZombie.attackEntityFrom): a hurt zombie with an attacker or a target may call a new zombie to a dark, clear spot 7 to 40 blocks away, which goes after the same target; the zombie.spawnReinforcements attribute (up to 0.1 at birth, +0.5 to 0.75 for leaders, -0.05 for caller and callee each time) is saved under Attributes with its modifiers. Zombie pigmen can call them too, as in the game
- `dd557a0` - Zombies break wooden doors (EntityAIBreakDoor): one in ten at most (by regional difficulty) and every leader zombie, saved as CanBreakDoors; it knocks (mob.zombie.wood), the crack grows over 240 ticks for every player within 32 blocks (S25PacketBlockBreakAnim, RenderGlobal.damagedBlocks) and on Hard the door breaks (mob.zombie.woodbreak, its particles, the door drops). Paths go through open wooden doors for every mob, and through closed ones for zombies and villagers (canPassOpenWoodenDoors defaults to true, setBreakDoors), as PathFinder does
- `9278cd4` - Compass and clock icons move (TextureCompass, TextureClock): the needle swings toward the world spawn sent as S05PacketSpawnPosition on login and after /setworldspawn, the dial turns with the celestial angle, both spin at random outside the Overworld; in the hotbar, the hand, on the ground
- `f209a27` - Chinese and other non-ASCII text in the world and from the keyboard: signs and name tags draw characters outside ascii.png (and every character when the Unicode font is on) from the Unicode glyph pages as renderUnicodeChar does, sign text takes the § colour codes, and the window accepts input-method text (Ime commit, as LWJGL's WM_CHAR) in chat, signs, books, anvils and the menus while staying off in play; `--setblock "x y z 63 m text=a|b|c|d"` writes a sign for screenshots
- `87856f4` - Box-built blocks no longer lose faces beside solid blocks: the test for a face lying on the block's edge looked at the wrong axis (x for the y faces, y for z, z for x), so an open door in a doorway lost its inner face and its edge whenever a wall stood next to it, and other partial blocks lost faces the same way; shotcompare scenes `doorway_s`, `doorway_n`, `doorway_close` (open doors in a plank wall from both sides and up close, now within 0.4% of the game)
- `de50f03` - renderBlockCauldron, renderBlockEndPortalFrame and renderBlockDragonEgg ported onto RenderBlocks: the cauldron's inner walls take the side texture unshaded and the water sits at getRenderLiquidLevel, the frame's top turns with its facing and the eye is a quarter-inset box with endframe_eye on every face (a facing-0 frame no longer shows a sliver), the dragon egg is its eight narrowing slices instead of a cube; shotcompare scene `cauldrons`
- `df60419` - Flower pots take plants (BlockFlowerPot.onBlockActivated: flowers, saplings, mushrooms, cactus, dead bush, fern into an empty pot; TileEntityFlowerPot saved as Item/Data, pots without one get their plant from the metadata as createNewTileEntity does, so the witch hut's red mushroom shows), the plant drops with the pot (not for a creative player), middle click picks the plant; renderBlockFlowerpot ported onto RenderBlocks (the pot as a standard block with smooth lighting, its inner walls and the dirt, the plant crossed at three quarters' size or the cactus as three boxes); shotcompare scene `pots`
- `046e09d` - Cauldrons work (BlockCauldron.onBlockActivated): a water bucket fills one (the bucket comes back empty), a glass bottle takes a level as a water bottle, dyed leather armour is washed for a level, and rain fills them one time in 20 where it rains (fillWithRain); the level change updates comparators
- `228259b` - The dragon egg teleports when clicked or used (BlockDragonEgg.tryTeleport, with the portal particles along its path), and redstone ore lights up when clicked or used, not only when walked on (onBlockClicked / onBlockActivated)
- `dedd248` - Placement as the blocks decide it: a torch goes wherever any wall or floor holds it (BlockTorch.canPlaceBlockAt; clicking the underside of a block or a side that cannot hold it puts it on the first wall that can, onBlockAdded), trapdoors hang on glowstone, slabs and stairs too (isValidSupportBlock, also for staying), levers, buttons, ladders, hooks and torches do not attach to a redstone block (isNormalCube), end portal frames face the player (onBlockPlacedBy)
- `5d2bf2e` - onBlockHarvested for doors (a creative player breaking the top half takes the lower one, so no door drops), piston heads (in creative the piston goes too) and tripwire (shears disarm it before it breaks, so the hooks do not fire)
- `2d14056` - Middle click as Minecraft.middleClickMouse: blocks are picked as Block.getItem / getDamageValue (either half of a door is the door item, stairs and torches lose their facing, crops give seeds, double slabs their slab, cocoa beans, skulls by type, nothing for spawners and portals), a hotbar slot holding it is selected in survival too, creative swaps it in from the inventory (creativePickItem); in creative, mobs give their spawn egg, carts, boats, paintings, leads and item frames their item. Partial blocks' north and east faces take their U backwards within the box (renderFaceZNeg / renderFaceXPos without field_152631_f), so door edges show the right strip of the texture; shotcompare scene `doors`
- `69714cb` - Spider jockeys and chicken jockeys: one spider in a hundred spawns with a skeleton on its back, a baby zombie (or zombie pigman) may ride a chicken near by or a new one (onSpawnWithEgg), riders sit at getMountedYOffset + getYOffset and follow the mount's turn; `--summon Spider:jockey`. Also a ghast's waypoint now starts at the origin like EntityGhast's (a waypoint on itself gave an infinite push and a hang)
- `238ce8d` - The Statistics screen's Blocks and Items pages (GuiStats$StatsBlock / $StatsItem: crafted, used and mined or depleted, crafted and used, with the slot icons and the column header), and the statistics behind them: mineBlock.<id> on harvesting, craftItem.<id> for crafted and smelted results, useItem.<id> when a held stack is used on a block, breakItem.<id> when a tool breaks
- `927ab54` The Statistics screen's Mobs page (GuiStats$StatsMobsList: each mob killed or killed by, with "You killed" and "killed you" lines), and stat.entityKilledBy.<name> counted on death
- `373e545` More achievements: When Pigs Fly (EntityPig.fall over 5 blocks with the player on it), On A Rail (a thousand blocks from where the minecart ride began) and The Beginning. (killing the Wither)
- `8469f01` Achievements unlock: picking up wood, leather, diamonds and blaze rods (EntityItem.onCollideWithPlayer), crafting a workbench, pickaxes, furnace, hoe, bread, cake, sword, enchanting table and bookshelf (SlotCrafting.onCrafting), smelting iron and fish (SlotFurnace), taking a brewed potion, killing a monster, a blow of 18 (Overkill), and going to the Nether, the End and back
- `7b8f29d` Breeding counts towards stat.animalsBred and unlocks Repopulation for cows (EntityAIMate.spawnBaby's triggerAchievement)
- `2ff7701` Minecarts rumble as they roll: MovingSoundMinecart (minecart.base repeating at the cart, its volume from the speed) and MovingSoundMinecartRiding (minecart.inside without attenuation while riding), with repeating sounds in the mixer
- `bb5c412` Enchanting tables draw glyphs from their bookshelves (BlockEnchantmentTable.randomDisplayTick, EntityEnchantmentTableParticleFX flying down into the table; the brightening as they arrive left out)
- `f16e1d2` Statistics: the server keeps playOneMinute, the walked, swum, dove, climbed, flown and fallen distances (EntityPlayer.addMovementStat, fall), jumps, drops, damage dealt and taken, deaths, mob kills and killEntity.<name>, sends them on REQUEST_STATS (everything the first time), and GuiStats shows its General page with the game's formats (time, distance, damage, grouped numbers); `--open stats`
- `e6c9c29` Pumpkins and heads worn in the helmet slot are drawn on zombies, skeletons, zombie pigmen and the player (renderEquippedItems: a 3D block at 0.625 under the head, a head through TileEntitySkullRenderer at 1.0625)
- `c3e0fe7` Wolves shake off water and tilt their heads when begging: EntityWolf's isWet / isShaking / timeWolfIsShaking (wet in water or rain, the shake once standing still on the ground, mob.wolf.shake and the splashes), setBegging from EntityAIBeg, the client's headRotationCourse, ModelWolf's getInterestedAngle and getShakeAngle on head, mane, body and tail, and RenderWolf's darker coat while wet
- `4a44159` The Achievements screen from the pause menu: GuiAchievements (the rock background with its random ores by depth from the player's id, the lines and arrows to each parent in grey, green or black, the icons in their frames shaded by how far they are from reach, locked items as silhouettes, dragging the map with easing, the tooltip with the description, "Requires" or the unknown name, and Taken!), clipped to the window; `--open achievements`
- `cf01112` Fire charges light fires (ItemFireball.onItemUse: fire on the clicked side with fire.ignite, used up outside creative)
- `898691c` Chests, trapped chests and ender chests held, dropped and in frames are drawn as the chest model (renderBlockAsItem type 22 through TileEntityRendererChestHelper); shadows no longer show bits of other textures (the quads stay inside shadow.png's square, as GL_CLAMP does)
- `3f1a042` Cocoa pods are drawn (RenderBlocks.renderBlockCocoa: the pod of each growth stage on its side of the log, its top and bottom, the stalk); shotcompare `cocoa` scene, 0.8% of pixels off
- `9da751e` The Wither: BlockSkull.makeWither (three wither skulls on a T of soul sand either way round, not on Peaceful, the spawnWither achievement, snow puffs), EntityWither (charging for 220 ticks while healing to full and growing, the blast of 7, flying after the player, the main head's skulls every 40 ticks, the side heads picking living targets that are not undead and firing every 40-60 ticks, blue skulls on Normal and Hard when idle, breaking out of blocks after being hurt, healing a point a second, smoke and spell particles, a nether star and 50 experience, Invul in NBT), EntityWitherSkull (8 damage, Wither II for 10 or 40 seconds, healing the wither for a kill, a blast of 1, the blue skull's slower flight), ModelWither and RenderWither (the blue flickering charge texture, the size, the armour glow at half health), RenderWitherSkull, and its boss bar
- `64584b0` The boss health bar: RenderDragon's BossStatus.setBossStatus and GuiIngame.renderBossHealth (name, bar, 100 frames after the boss was last drawn)
- `d5d4204` Lightning: WorldServer's thunder (one chunk in 100000 a tick while thundering, where rain can fall), EntityLightningBolt (the thunder and the blast, fire on Normal and Hard, restrikes, what is within 3 blocks: 5 fire damage, a charged creeper, a pig to a zombie pigman with a golden sword, a villager to a witch), the client's own bolt with the sky flash (World.getSkyColor) and full sky light (EntityRenderer.updateLightmap), RenderLightningBolt's layered branches drawn added, charged creepers (powered in NBT, a blast of 6, RenderCreeper's swirl); `--summon LightningBolt`
- `f7dd761` Books: GuiScreenBook for the book and quill and written books (pages up to 50 wrapped to 116 pixels and 256 characters, the blinking cursor, page turners, Sign / Done, the title screen with Finalize / Cancel and its warning, Escape closing without sending), MC|BEdit and MC|BSign (dropping empty last pages, the author and title, becoming a written book), the written book's title as its name and "by" line
- `78d0650` Head selection boxes (BlockSkull.setBlockBoundsBasedOnState on the floor and the four walls)
- `6544d96` Mob heads: BlockSkull (no drop after a creative player's hit), ItemSkull.onItemUse (on the floor turned in sixteenths with the player, or on a wall), TileEntitySkull (SkullType, Rot in NBT, sent with the chunk and on placing) and TileEntitySkullRenderer's ModelSkeletonHead with the skeleton, wither skeleton, zombie, Steve and creeper textures
- `6a977cb` Beacons: BlockBeacon (renderBlockBeacon's glass, obsidian and core, light 15, the item drawn as render type 34), TileEntityBeacon (the pyramid of iron, gold, emerald and diamond blocks checked every 80 ticks under an open sky, the powers of each tier for players within levels * 10 + 10, Strength / Regeneration and the level-4 secondary or doubled primary, the fullBeacon achievement, Primary / Secondary / Levels in NBT), ContainerBeacon with its payment slot and shift-click rules, MC|Beacon, GuiBeacon (the tier buttons, done and cancel, the payment icons, the tooltips) and TileEntityBeaconRenderer's turning inner beam and faint outer square
- `2156335` The enchantment glint in the world: the two scrolling layers of ItemRenderer.renderItem / RenderItem (colour 0.38, 0.19, 0.61, texture matrix scale 0.125, the 3000 and 4873 ms cycles, -50 and 10 degrees) over held items in first person, dropped items and items in frames, drawn with glBlendFunc(GL_SRC_COLOR, GL_ONE) and GL_EQUAL depth so only the item's own pixels shine
- `ce09e61` Leads: EntityLiving.interactFirst (tie a mob that allowLeashing, a tamed one by its owner; untie and drop the lead), ItemLead.attachLead and EntityLeashKnot on fences (tying, untying, creative removal, the knot not saved and made again from the mob's Leash tag, falling off without a fence), EntityCreature.updateLeashedState (a home of 5 around the holder with EntityAIMoveTowardsRestriction, walking after it past 4, pulled past 6, breaking past 10 or when the knot goes, a sitting pet only breaking), the home check in RandomPositionGenerator, Leashed / Leash in NBT, RenderLiving.renderLeash's two sagging strips and RenderLeashKnot
- `ba0e1be` Maps: ItemEmptyMap (a new map at scale 0 around the player, World.getUniqueDataId), ItemMap.updateMapData (a sixteenth of the columns a tick, the most common MapColor of each pixel from every block's getMapColor as the game gives it in tools/MapColorHarness.java, height shading, water depth, the Nether's pattern), the player's marker and the off-map pointer, data/map_N.dat and idcounts.dat, maps made for a stack whose map is missing (scale 3 at spawn), RecipesMapExtending (a new map one scale up), and the client: MapItemRenderer's texture in the atlas, the map held in both hands (ItemRenderer.renderItemInFirstPerson, lower as one looks up) and maps in item frames (renderFrameForMap and the four rotations)
- `b75296c` Horses, donkeys and mules: EntityHorse's taming (temper from feeding, EntityAIRunAroundLikeCrazy bucking the rider off or taming with hearts), saddles, armour and donkey / mule chests through the AnimalChest and GuiScreenHorseInventory, riding (steering, the charged jump with the jump bar in place of the experience bar, rearing), breeding (mules, averaged speed, health and jump), eating, mouth and tail animations, the full ModelHorse (saddle, straps, reins while ridden, chests, foal scaling), the armour layer, zombie and skeleton horse textures, the mount's hearts in place of the food bar, and horse NBT
- `7b56633` Villager trading: EntityVillager's offers by profession (villagersSellingList / blacksmithSellingList, enchanted books and tools, adjustProbability, one new offer at a time, Collections.shuffle), MerchantRecipe(List) with canRecipeBeUsed and addToListWithCheck, ContainerMerchant / InventoryMerchant / SlotMerchantResult (both price orders, shift-click retrying, prices back on close), useRecipe (seven uses, wealth, the yes/no sounds), the restock 40 ticks after the last offer is used (more uses for spent offers, a new offer, Regeneration), EntityAITradePlayer and EntityAILookAtTradePlayer, Offers and Riches in NBT, and GuiMerchant (buttons, the offer's items and tooltips, the crosses on a spent offer)
- `3af8b24` Entity shadows: Render.renderShadow with fancy graphics (shadow.png on the tops of full blocks under the entity, fading with height, light and camera distance) for every mob with its renderer's shadowSize (children halved, the baby villager's quirk), dropped items and experience orbs (0.15 at 0.75), boats, minecarts, primed TNT and falling blocks
## 2026-11-23

- Launch hand-thrown projectiles and bow shots near the player's eyes instead of at their feet
- Fix clipped potion icons, hotbars, and tooltips on crowded inventory screens by growing the GUI vertex buffer and batching neighboring glint pixels
- Fix inventory pointer alignment at odd window sizes, use the current slot for shortcuts, include crossed slots in fast drags, and preserve held items during creative scrolling, tab clicks, and empty panel clicks
- `e6bc545` Distant terrain and the night vignette as in the game: the terrain's first pass cuts out at alpha 0.5 (EntityRenderer.renderWorld's glAlphaFunc), which removes the dark lines along grass edges and the bright far-off flowers; mipmaps follow TextureUtil.generateMipmapData (transparency judged by the first pixels only, transparent averages over all four); the client's skylightSubtracted stays at its value from the world's creation (WorldClient never updates it), so the vignette, the fog's dimming and minecart brightness no longer darken at night (shotcompare: above 7.1% -> 1.7%, snowtrench 9.9% -> 0.4%, the block scenes about 3% -> 1.5%)
- `5c52e15` No arm swing when right-clicking a mob or vehicle (interactWithEntitySendPacket) or dropping an item (dropOneItem); the bow's pulling frames change on getItemInUseCount, not a tick early
- `37dc60d` Selection boxes, arm swing and the hand drop as in the game: every block's selection box checked against RenderGlobal.drawSelectionBox (tools/SelectionHarness.java -> tools/selection_1710.txt; snow layers, saplings, grass, flowers, mushrooms, cactus, sugar cane, soul sand, end portal, extended pistons and heads, rails, pressure plates, vines, cocoa, tripwire and hooks were wrong); swingItem starts at -1 with getArmSwingAnimationEnd (Haste, Mining Fatigue) and getSwingProgress wraps forward, so a swing lasts its six ticks; right-clicking swings only when onPlayerRightClick did something, and the hand drops (resetEquippedProgress) when the stack changes size, in creative, or after throwing a potion, snowball, egg, pearl, bottle or eye; switching between two slots with the same item drops the hand too; shotcompare `snowtrench` scene
- `8a58089` Fireworks: RecipeFireworks (rockets with Flight and their stars, stars with colours, shape, trail and twinkle, fade colours added), ItemFirework.onItemUse and the dispenser's rocket, EntityFireworkRocket (lifetime, the climb, the spark trail, saved as FireworksRocketEntity), the client's EntityFireworkStarterFX (small and large balls, star, creeper face, burst, the blast / largeBlast / twinkle sounds near and far), EntityFireworkSparkFX (fading, fade colours, trails, twinkling, full brightness, drawn blended) and EntityFireworkOverlayFX; `--summon Fireworks:shape:colour:fade:life`
- `a7931c4` Snow golems and building golems: BlockPumpkin.onBlockAdded (a pumpkin or jack o'lantern on two snow blocks, or on a T of iron blocks either way round, with the client's snowshovel / snowballpoof particles; iron golems built this way are player-made), EntitySnowman (EntityAIArrowAttack with snowballs, the nearest monster it can see as target, hurt by water and by biomes above 1.0, a snow trail below 0.8, never despawns, nextInt(16) snowballs), ModelSnowMan and RenderSnowMan's pumpkin, EntitySnowShovelFX; shotcompare `golems` scene
- `a342d4b` Trapped chests and ender chests: BlockChest with its kind (doubles pair only the same kind, canPlaceBlockAt refuses a third), the trapped chest's power while open (weak 1, strong upwards) and TileEntityChest.openInventory's neighbour updates, BlockEnderChest (opens InventoryEnderChest unless covered, EnderItems in the player's NBT, light 7, eight obsidian, portal particles), their renderers, GUI icons and lids; shotcompare `chests` scene
- `15bf76b` Paintings and item frames: ItemHangingEntity, EntityHanging (setDirection, onValidSurface every 100 ticks, breaking by hits and explosions, drops), EntityPainting (a random art that fits, Motive), EntityItemFrame (put an item in, turn it, the first hit takes the item out, CustomName label of a renamed item), saved as Painting / ItemFrame, RenderPainting and RenderItemFrame; orbs can no longer be targeted (canBeCollidedWith); shotcompare `hanging` scene and `--hang`
- `ef275be` Name tags: ItemNameTag.itemInteractionForEntity (only a renamed tag; the mob is named and kept from despawning; a tamed wolf or cat also toggles sitting), renamed spawn eggs name their mob (by hand and from dispensers), CustomName in NBT, and RendererLivingEntity.renderLivingLabel above the mob under the crosshair (backing and faint text through walls, then the white text)
- `a75cb1e` Pigs can be saddled and ridden: ItemSaddle, EntityPig.interact, EntityAIControlledByPlayer (steering, speed ramp, step-up jump, stick wear turning it back into a fishing rod), ItemCarrotOnAStick boost, sneaking off, the saddle layer (ModelPig 0.5), Saddle in NBT and the saddle drop
- `a3217ee` Jukeboxes play records: ItemRecord.onItemUse and BlockJukebox (insert, eject with the original offsets, break, comparator signal id - 2256 + 1, saved as RecordPlayer), the record streams from the jukebox with distance and pan following the listener, and "Now playing" shows

## 2026-11-21

- Keep unsaved chunks available for retry and preserve existing Anvil chunk data until the region index points to the replacement
- `03a396a` Dying mobs no longer twitch: deathTime, and the swing, creeper fuse, chicken wing, slime squish, squid, sheep eating, golem attack counters and the age, advance on the client's own ticks (they came from the server out of phase with the client partial; per-frame logging: death angle 13 reversals in one death -> 0, age 12% of frames -> 0)
- `cfac1b8` Daylight sensors: BlockDaylightDetector and its tile entity (every 20 ticks the sky light scaled by the sun's angle, power from the metadata, none in the Nether, saved as DLDetector)
- `02854fa` Redstone comparators: BlockRedstoneComparator and TileEntityComparator (compare and subtract, reading containers through Container.calcRedstoneFromInventory, double chests, cakes, cauldrons, end portal frames, container minecarts on detector rails, through a solid block), renderBlockRedstoneComparator, saved OutputSignal; containers wake comparators when their slots change; repeaters are now locked by comparators and prioritise behind them as in isRedstoneRepeaterBlockID; droppers face away from the player when placed; shotcompare `comparator` scene

## 2026-11-20

- `25f0b84` Creative players break bedrock and other unbreakable blocks again, as ItemInWorldManager does (the hardness check refused them since 5be2c2f)
- `db1b5cb` Mobs outside the view frustum are not built (RenderGlobal.renderEntities' isBoundingBoxInFrustum, the dragon excepted): frame building 0.57 -> 0.34 ms at distance 16, about 930 -> 1,050 fps; all 12 shotcompare scenes re-run, no regressions
- `4a3579c` World generation matches 1.7.10 again: lava springs placed during decoration hissed with the generator's random instead of world.rand (since 7fd9193), shifting every later ore, silverfish stone, emerald and lake; --compare-pop is 0 differences on three seeds again
- `63889bb` Hoppers move items (TileEntityHopper: pull from above or pick up items, push into the faced container, sided furnace and brewing-stand slots, double chests, chest and hopper minecarts, 8-tick cooldown, off while powered); droppers push into containers; dispensers get every Bootstrap behaviour (projectiles, spawn eggs, fire charges, boats, buckets, flint and steel, bone meal, TNT), smoke, the 1.7.10 drop height and the tile's own random; pistons no longer push signs, brewing stands or enchanting tables; shotcompare `hopper` scene
- `9159038` Buckets (ItemBucket: fill from sources, pour, water fizzes in the Nether) and spawn eggs (ItemMonsterPlacer on blocks and into liquids), both missing until now; hopper and dropper blocks with renderBlockHopper, collision boxes, windows and saving (transfer logic next)
- `c82c142` Weighted pressure plates (light and heavy: entity count, tick rate 10) and stained glass panes; renderBlockStainedGlassPane and renderBlockPane converted from the Java; panes join stained glass and each other (canPaneConnectToBlock), pane items and break particles show the glass; thrown items, falling blocks and the fishing hook count on plates; shotcompare `panes` scene
- `1e7b8f1` Fishing: ItemFishingRod and EntityFishHook (casting, float, lure and weather timing, bites with wake and splash particles, hooking mobs, boats and minecarts, junk/treasure/fish tables with Luck of the Sea, XP orb, rod wear, fishing statistics); RenderFish bobber and line, cast rod icon, EntityFishWakeFX; MINERUST_USE_AIR for captures
- `4f88e09` renderBlockStairs and renderBlockFence ported (smooth lighting, field_152631_f, the lone post's hidden bar); shotcompare `stairs` scene
- `8ae53ef` Slabs, snow layers, buttons, pressure plates, carpet, trapdoors, farmland and enchanting tables drawn by renderStandardBlock (smooth lighting, BlockSlab culling); renderBlockCactus ported; no block outline while F1 hides the GUI; shotcompare `shapes` scene
- `d051db5` RenderBlocks ported line by line for torches, repeaters (locked-repeater bedrock bar, diode top), ladders, rails and crops; shotcompare `redstone` scene with `--setblock` in ShotHarness
- `c7262c2` First-person arm: ModelBiped idle sway (0.1 rad), EntityPlayerSP arm lag, standard item lighting; F3 screen ported line by line (render chunk, entity, particle, chunk cache and memory lines, vanilla positions and colours); shotcompare f3 scene
- `c5269b8` Spawn area prepared like MinecraftServer.initialWorldChunkLoad (vanilla decoration order: same trees near spawn); statistics file and achievement tree, inventory hint and "Taking Inventory" (GuiAchievement); shotcompare --debug
- `678e5ae` Screenshot comparison with the real 1.7.10 client (tools/shotcompare.py, tools/ShotHarness.java); direct launches take the render distance from options.txt; unattended runs never take focus or the mouse; --hide-gui
- `9c5fd64` Renderer moves to Vulkan 1.3: dynamic rendering instead of render pass and framebuffers, synchronization2 barriers and queue_submit2 (screenshots pixel-identical, same fps)
- `18842ad` Run cargo fmt over the last commits
- `5922f5b` Reorganise the docs: PARITY_TODO is the only to-do list, the dev log moves to CHANGELOG, stale and contradictory lines fixed
- `c57b73a` Mobs push each other and the player (collideWithNearbyEntities)
- `2a9a022` Client body follows the head for task-AI mobs (EntityBodyHelper); head clamp in the old look helper
- `7ddace6` Mobs move through Entity.moveEntity and onEntityUpdate: steps, splashes, falls, drowning

## 2026-11-19

- `a78ceb6` Fire damage from fire blocks and cactus on the server; burning entity overlay
- `9cf1260` Zombie villagers: spawn, infection, golden-apple cure, model
- `5dc208a` Mobs pick up loot; baby zombies; mob sound volume and pitch
- `95d5173` Draw dropped items as RenderItem.renderDroppedItem does
- `c894638` Give mobs potion effects; helmets keep the undead from burning
- `a00e95e` Make the particles and graphics options take effect
- `e332011` Relight chunks from scratch once they are final; fix dark patches

## 2026-11-15

- `ddd8fd8` Fix TNT blast hitches and slow exit saves; add the vignette
- `a004173` Audit player melee and damage intake; anvils hurt; mob Thorns

## 2026-11-14

- `064b640` Upgrade zip to 9 for the three advisories against 2.x
- `7fd9193` Audit the random ticks, drops, liquids and mob melee against the original
- `7083356` Enchantment glint on items in the GUI
- `0ef98e8` Mob equipment: armour, weapons and bows from onSpawnWithEgg
- `7a1bc19` Farming, beds, cake, fence gates, walls and stems
- `8cc7329` Enchantment effects, server-side melee and per-class mob loot
- `c6294d6` Slimes: prevSquishFactor after the new factor; vulnerable-player search
- `7806964` Breeding: love mode, EntityAIMate and the young
- `d586cc3` Hang wolves, ocelots and iron golems on their original tasks
- `fa88c36` Port the mob AI: EntityAITasks, PathNavigate and the old creature AI
- `d72a429` HANDOFF: correct the test count
- `5b707d4` Place stairs, slabs and other blocks as the original does; pick by shape
- `50d671b` Add stained glass, wooden slabs and quartz stairs
- `495f9a1` Predict block digs and placements on the client; armour, blocking, crits
- `23aa018` Release the mouse for screens the server opens; port the lever renderer

## 2026-11-13

- `5e33448` Port block drops; redstone wire, primed TNT, creeper ignition, placement check
- `439ad22` Interpolate server entities on the server clock; fix block textures
- `bed27ea` Render chunks per 16-cube section; pack them in a vertex arena
- `e87f170` Separate Math.random; reference block drops from the original
- `d35534f` Load the jar from <repo>/game; search MSIX LocalCache roots
- `bde39ff` Find the jar without relying on environment variables
- `4455023` HANDOFF: fix mangled paths
- `a5571af` Find the game jar without arguments; show startup errors in a dialog
- `a65e3cb` Block items in 3D: renderBlockAsItem
- `1a3b948` Anvil: block, ContainerRepair, GuiRepair
- `3239bf6` Give item stacks their NBT tag; armour dyeing
- `cc422c5` Add the enchanting table: ContainerEnchantment, GuiEnchantment and the book
- `5de6c71` Port EnchantmentHelper, add the enchanting table block and a handoff guide
- `893a6c1` Add signs: blocks, ItemSign, TileEntitySign, renderer and GuiEditSign
- `0f958b9` Add the chat settings and loading screens; dispensers open
- `d89b492` Draw armour and held items on the player; armour can be worn
- `19c00c4` Add chat and commands, F5 third person, key bindings in game
- `ce8477f` Add GuiContainerCreative and the chest tile entity renderer
- `9991930` Fix the title panorama, mob facing and jitter, debris tints; add the inventory player
- `b51da64` Port Container.slotClick, crafting recipes, armour slots and item tooltips
- `d5cca42` Add the title screen and the option menus
- `064e332` Port EntityXPOrb and the experience sources

## 2026-11-12

- `658e0bf` Port Explosion
- `8757c1e` Port EntityMinecart and its subclasses
- `ca37da9` Port EntityBoat, riding and EntityTrackerEntry
- `94ee298` Port EntityThrowable: snowballs, eggs, ender pearls and splash potions
- `2da10dc` Add the brewing stand
- `cd13ab3` Draw potions in two passes and the active effects beside the inventory
- `1f2abbb` Port item use, ItemFood, the bow, EntityArrow and GuiIngame.renderIcons
- `08396b6` Move the player's survival state to the server (EntityPlayerMP)
- `0bffc5a` Port Potion, PotionEffect, PotionHelper and the EntityLivingBase core
- `50a56bc` Port RenderEndPortal, BlockPortal shape and EntityFallingBlock
- `9e5981d` Port TileEntityPiston, RenderBlocks standard blocks, Entity base and EntityItem
- `7c4c82d` Update README and bug audit for redstone, mobs, Nether, End and Anvil
- `2820f3a` Anvil: open, save and export vanilla 1.7.10 worlds
- `9c4193f` Nether and End mobs: ghasts, zombie pigmen, blazes, magma cubes, wither skeletons, the ender dragon
- `7221603` Nether and End: generation, fortresses, fire, portals and dimension travel
- `ab05323` Add slimes, endermen, squid, bats, wolves, ocelots, horses and iron golems
- `f34d912` Redstone: power, wire, torches, repeaters, levers, buttons, plates, lamps, pistons, TNT, notes, dispensers, doors
- `f1cb5ae` Add particles (block debris, smoke, flame, lava, drips, rain splashes, explosions, crits, running dust) and the underwater overlay
- `a21f79e` Fix MC-1538 (sound clipping) and MC-12918 (structure chest facing); update bug audit for structures, mobs, sound and GUI
- `a889629` Add weather: rain and thunder cycle, darker sky, fog, clouds and lightmap, rain and snow rendering, snow accumulation, rain sound
- `848bd34` Add sound: pure Rust output and Vorbis decoding, sounds.json events, block, step, mob, bow, explosion and music sounds
- `cd434ba` Add mobs: animals, monsters, villagers and witches with vanilla models, pathfinding, AI, combat, drops, experience, world-gen and natural spawning, arrows and creeper explosions
- `bbf1d40` Add structure generation: villages, mineshafts, strongholds, desert and jungle temples, witch huts
- `5be2c2f` Add survival gameplay: mining with break times and crack overlay, drops and pickup, inventory and crafting, furnaces and chests, health, hunger, air, fall and lava damage, death and respawn, torches, first-person hand, HUD and menus

## 2026-11-11

- `c8a82fe` Add GUI rendering: font, hotbar with block icons, F3 overlay, slot selection
- `86278a9` Add random block ticks: grass and mycelium spread, leaf decay, melting, cactus, reeds, mushrooms, vines, natural freezing
- `4e3ded2` Add fancy clouds, stars, sunrise glow, animated water and lava, water currents
- `80a98ac` Minecraft 1.7.10 rewritten in Rust on Vulkan
