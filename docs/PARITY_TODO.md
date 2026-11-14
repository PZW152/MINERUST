# 與原版 1.7.10 的差距清單

原則：照 `src-java/net/minecraft/` 的原始類別逐函式移植，不做任何「近似」或「簡化」。
做完一項就從這裡刪掉並寫進 README；這份清單只列還沒做到 100% 的部分。

## A. 先前被當成「刻意差異」的簡化（優先）

- [x] 活塞：`BlockPistonMoving` + `TileEntityPiston`（兩 tick 的移動、推動實體、`TileEntityRendererPiston` 動畫）、
      `World.addBlockEvent` 佇列
- [ ] 終界傳送門：`TileEntityEndPortalRenderer`（16 層星空視差）
- [x] `Potion` / `PotionEffect` / `PotionHelper` 與玩家身上的效果（凋零、中毒、抗性、速度、跳躍、生命提升、吸收……）
- [x] 藥水物品（飲用）、釀造台（方塊、`TileEntityBrewingStand`、介面、存檔）、牛奶、金蘋果
- [x] `EntityThrowable`：雪球、蛋（孵小雞）、終界珍珠、噴濺藥水（`playAuxSFX(2002)`、`EntitySpellParticleFX`），女巫照原版選藥水並投擲
- [ ] 生物身上的持續藥水效果、玻璃瓶裝水、幼年生物的大小與模型
- [x] 容器點擊：`Container.slotClick` 七種模式（拖曳分配、雙擊收集、數字鍵交換、Q 丟出、創造複製）、`mergeItemStack`、
      各容器的 `transferStackInSlot`、`SlotCrafting` / `SlotFurnace` / 盔甲欄 / 釀造欄；客戶端以同一套程式預測
- [x] 合成：`CraftingManager` 的 317 條配方（原版順序、鏡像、萬用中繼值）、工具修復、地圖複製
- [x] 盔甲欄（存檔 Slot 100–103）、精確的 `addItemStackToInventory`、關閉介面時合成格與游標物品被丟出
- [x] 物品名稱（全部語言）、稀有度顏色、`getTooltip`（攻擊傷害、附魔、藥水效果、唱片）、多行 `drawHoveringText`
- [x] 背包畫面的玩家模型（`GuiInventory.func_147046_a`，隨滑鼠轉向）
- [x] 生物：`EntityTrackerEntry`（1/32 位置、4/32 門檻、`S19` 頭部）與客戶端三步內插、`updateDistance` 身體朝向、
      客戶端肢體擺動
- [ ] 方塊破壞時容器內容物的散落方式
- [x] 物品 NBT（附魔、名稱、RepairCost、染色、Lore）與皮甲染色配方
- [ ] 特殊配方：煙火、書本複製、地圖擴大
- [x] 創造模式物品欄（12 個分頁、搜尋、捲動、背包分頁與垃圾桶、`C10PacketCreativeInventoryAction`）
- [x] 聊天（`GuiNewChat` / `GuiChat`）與指令：help、give、time、gamemode、tp、weather、toggledownfall、kill、
      effect、xp、seed、difficulty、clear、setblock、spawnpoint、setworldspawn、say、me
- [x] F5 第三人稱（前後兩種）、F1 隱藏介面、遊戲中的按鍵全部改讀按鍵設定
- [x] 玩家模型（第三人稱與背包）上的盔甲層、手持物品，`ModelBiped.setRotationAngles` 完整移植；右鍵穿上盔甲；南瓜頭的視野遮罩
- [x] 箱子：`TileEntityChestRenderer`（單／大箱子、開蓋動畫與音效、聖誕節材質），箱子物品圖示
- [x] 聊天設定畫面、載入畫面（`LoadingScreenRenderer` / `GuiDownloadTerrain`）、發射器介面
- [x] 附魔台：`ContainerEnchantment`、`GuiEnchantment`（書本、銀河字母）、`TileEntityEnchantmentTableRenderer`；
      `EnchantmentHelper.buildEnchantmentList` 與附魔台等級和原版逐筆一致
- [x] 附魔效果：鋒利／不死剋星／節肢剋星、擊退、燃燒、掠奪、保護四系列與摔落保護、水下呼吸、親水性、荊棘、效率、
      耐久（工具、武器、盔甲、弓、剪刀、打火石）、力量／衝擊／火焰／無限（`tools/EnchantFxHarness.java` 比對）
- [ ] 附魔效果尚缺：海之眷顧與魚餌（釣魚未實作）、節肢剋星的緩速（生物還沒有藥水效果系統）、附魔台書架粒子
- [x] 生物死亡掉落：每種生物的 `dropFewItems`（含掠奪）與 `dropRareDrop` 擲骰，與原版 3,168 組一致；
      終界使者不再掉落手上的方塊（1.7.10 本來就不掉）
- [x] 生物受傷：天生護甲（殭屍、殭屍豬人 2 點）、`setBeenAttacked` 與擊退抗性的亂數、擊退方向改用 `Math.random()`
- [ ] 生物裝備（`EntityLiving.equipment`、`addRandomArmor`、`enchantEquipment`、手持弓劍、掉落裝備）與
      `onSpawnWithEgg` 的其餘部分（殭屍擊退抗性加成、跟隨距離的 `nextGaussian`、蜘蛛騎士、小殭屍）
- [ ] 生物的藥水效果（`activePotionsMap`：飛濺藥水的持續效果、節肢剋星緩速、凋零骷髏的凋零）
- [ ] 物品附魔光澤、手持箱子與掉落箱子的模型
- [ ] 鋤頭耕地（`ItemHoe.onItemUse`）尚未實作
- [ ] 指令：summon、enchant、gamerule、playsound、spreadplayers、testfor 等，Tab 自動補完
- [ ] 統計與成就（`StatList` / `AchievementList`）與其畫面；資源包、使用統計（Snooper）、直播畫面
- [x] 船：`EntityBoat` 兩端物理（客戶端預測、`isBoatEmpty`）、`ItemBoat`、`interactFirst` / `mountEntity` /
      `dismountEntity`、`updateRiderPosition`、撞毀成木材與木棍、摔落、`EntityTrackerEntry` 封包（`S0E` / `S14` / `S18` /
      `S12` / `S1C` / `S1B`）、`getBoundingBox` 讓所有實體與船相撞、`RenderBoat`、「按 LSHIFT 下來」提示
- [x] 方塊上右鍵沒有被方塊或 `onItemUse` 接手時改走 `onItemRightClick`（`Minecraft.func_147121_ag`）
- [x] 礦車：`EntityMinecart` 兩端（伺服端 `moveOnRail` / `moveOffRail` / `applyDrag`、客戶端 `turnProgress` 內插）、
      一般／箱子／動力（煤、推力）／TNT（引信、撞擊與摔落爆炸、`setEntityState(10)`）礦車、`ItemMinecart`、
      礦車互撞與推動生物（空礦車載走生物）、偵測鐵軌、`RenderMinecart`（沿鐵軌的傾斜與 `getPosOffset`、顯示方塊）、
      礦井的箱子礦車、存檔
- [ ] 礦車：漏斗礦車的吸取（等漏斗方塊）、指令方塊與生怪磚礦車、觸發鐵軌（157）、`MovingSoundMinecart` / `...Riding` 的連續音效、騎乘中的生物存檔（`Riding`）
- [ ] 騎乘：馬（跳躍、鞍、馬鎧、箱子、馬的物品欄）、豬＋鞍＋胡蘿蔔釣竿
- [ ] 傳送門建造時的中繼值與原版一致
- [ ] 實體（生物、掉落物、箭、礦車）穿越傳送門 `Entity.travelToDimension`
- [ ] 落沙／礫石／鐵砧／龍蛋的 `EntityFallingBlock`
- [ ] 雙層植物上半燒掉時的掉落行為放到 `--vanilla-parity` 開關後
- [x] 掉落物：`EntityItem` 逐行重寫（`pushOutOfBlocks`、水流、岩漿燃燒、合併、撿拾音效與飛向玩家的動畫）
- [x] `EntityArrow` 逐行移植（`World.rayTraceBlocks`、`collisionRayTrace` 含樓梯八格、`calculateIntercept`、撿回、燃燒箭點燃 TNT）
- [x] `Explosion` 逐行移植：世界亂數的射線、`exploder` 的抗性鉤子（點燃的 TNT 礦車保住鐵軌）、Java 8
      `HashSet<ChunkPosition>` 的迭代順序、`getBlockDensity`、所有實體（玩家眼高 0.12、創造模式也會被推、掉落物、
      船、礦車、箭、投擲物、落沙）、`doExplosionB(false)` 的掉落機率與連鎖 TNT、客戶端 `doExplosionB(true)` 的粒子；
      終界水晶的爆炸改回不起火
- [x] 方塊掉落表：`src/drops.rs`，與原版 4 萬多筆參考資料一致；時運與絲綢之觸生效
- [ ] 掉落的其餘路徑改走 `drops.rs`：`harvestBlock` 覆寫（剪刀、冰、雪片）、水沖與失去支撐、樹葉腐爛
- [x] 生物 AI 第一階段：`EntityAITasks` 任務系統與舊式 AI（殭屍、骷髏、苦力怕、女巫、豬、牛、哞菇、羊、雞、村民、蜘蛛、洞穴蜘蛛、蠹魚、殭屍豬人）
- [x] 生物 AI 第二階段：狼、豹貓、鐵魔像的任務
- [x] 繁殖（`EntityAIMate`、求愛、幼體大小與模型、存檔）
- [ ] 生物 AI 剩餘：馬；村民的交易／門／村莊；終界使者、烈焰使者、史萊姆、惡魂、蝙蝠、魷魚、蠹魚鑽石頭對照原始碼
- [x] 客戶端預測破壞方塊與放置普通方塊；[ ] 有朝向或需依附的方塊的預測
- [x] 玩家護甲值與護甲耗損、舉劍格擋減傷、爆擊 ×1.5
- [x] 紅石線照 `renderBlockRedstoneWire` 重寫（連接規則、爬牆段）；點燃的 TNT 白色閃爍；苦力怕打火石點燃
- [ ] 苦力怕：閃電劈中變成高壓苦力怕（爆炸半徑 6）、`mobGriefing` 規則
- [x] GUI 漸層改為真正的頂點漸層（`drawGradientRect`）

## B. 缺少的方塊

床、蛋糕、染色玻璃片、南瓜梗／西瓜梗、柵欄門、終界箱、指令方塊、信標、圓石牆、頭顱、陷阱箱、測重壓力板、比較器、
陽光感測器、漏斗、觸發鐵軌、投擲器

- [x] `Command::Use` 帶上點擊位置（`hitX/Y/Z`）：半磚上下半與合併（`ItemSlab`）、樓梯朝向與倒放、活板門上下、梯子、南瓜、藤蔓、絆線鉤、石英柱、乾草捆
- [x] 客戶端以 `World.rayTraceBlocks` 選取方塊，選取框為方塊實際範圍

另外：可可豆（目前不繪製）、箱子模型與開蓋動畫（`TileEntityChestRenderer`，目前是方塊）

## C. 物品行為

盔甲（穿戴、護甲值、耐久、渲染）、食物（全部）、藥水／噴濺藥水／玻璃瓶、桶（水、岩漿、牛奶）、骨粉、鋤頭、剪刀、
釣竿、終界珍珠、終界之眼、火焰彈、煙火、地圖、指南針、時鐘、鞍、拴繩、
命名牌、生怪蛋、書與筆／完成的書、唱片與唱片機、畫、物品展示框、床、告示牌、蛋糕、門、紅石中繼器／比較器物品

## D. 實體

畫、物品展示框、拴繩結、終界珍珠、終界之眼、
浮標、煙火、經驗球、噴濺藥水、凋零怪與凋零頭、閃電、雪人、巨人、殭屍村民、小殭屍、雞騎士、蜘蛛騎士、
生物穿戴裝備與撿拾物品、村民交易與繁殖、動物繁殖與幼年

## E. 系統

- [x] 玩家生存狀態移到伺服端（`EntityPlayerMP`）：`FoodStats`、傷害流程（難度縮放、護甲、抗性、吸收、擊退）、`CombatTracker` 死亡訊息（`en_US.lang`）
- [x] 物品使用（`setItemInUse`：吃、喝藥水、牛奶、拉弓、劍格擋的動畫），全部 `ItemFood` 數值與效果
- [x] 劍格擋的減傷（`isBlocking`）
- [x] 經驗球 `EntityXPOrb`：兩端的 `onUpdate`（飛向最近玩家，伺服端以腳下＋0.12、客戶端以眼睛＋0.12 為目標）、
      `getXPSplit`、`xpCooldown` 撿拾、`S11` 生成與追蹤（160 格、每 20 tick）、`RenderXPOrb`、撿拾動畫與音效、存檔；
      來源：生物死亡第 20 tick（幼體不給，動物用世界亂數）、終界龍、礦石（`dropXpOnBlockBreak`）、熔爐取出
      （`SlotFurnace.onCrafting`，含完整的 `FurnaceRecipes` 與經驗表）、經驗瓶 `EntityExpBottle`
- [ ] 經驗來源：繁殖、釣魚、村民交易；生物裝備的額外經驗
- [ ] 難度設定與 `level.dat` 的 `Difficulty`、遊戲規則

附魔（附魔台、所有附魔效果）、釀造、信標、成就與統計、指令與聊天、遊戲模式（創造、冒險、
極限）、難度、世界建立選單、選項選單、資源包、語言、地圖資料、終界詩與製作人員名單、多人與區域網路（協定 5）

## F. 渲染

- [x] 伺服器模擬的實體以伺服器 tick 時間插值（掉落物、箭、TNT、落沙、活塞），掉落物高度照 `RenderItem.doRender`
- [x] 方塊貼圖與原版 `Block.getIcon` 全表比對（`tools/icons_1710.txt`）
- [ ] 掉落物改為客戶端自行模擬（原版客戶端也跑 `EntityItem.onUpdate`，伺服器每 20 tick 校正一次）
- [x] 渲染區塊以 16³ 為單位（`WorldRenderer`），方塊變動只重建 `markBlockForUpdate` 範圍內的分段
- [x] 影像設定生效：垂直同步、最大幀率（`Display.sync`）、平滑光照等級、雲開關、視角晃動、Mipmap 層級、全螢幕（F11）
- [ ] 影像設定尚未生效：畫質「流暢」（不透明樹葉、平面雲、平面掉落物、無暈影）、粒子數量、各向異性過濾、
      3D 立體、進階 OpenGL、FBO；平滑光照關閉時非 `rb` 路徑的方塊（樓梯、柵欄、液體等）

- [ ] 實體陰影與著火（`Render.doRenderShadowAndFire`）
- [ ] 騎乘時玩家模型的坐姿（`ModelBiped.isRiding`，第三人稱與物品欄）
- [ ] 物品欄裡的玩家模型（`GuiInventory.func_147046_a`）
- [ ] 物品名稱與說明一律由 `en_US.lang` 取得（藥水前綴與後綴、附魔書等）

- [x] `RenderBlocks.renderStandardBlock`（含兩種平滑光照與六個 `renderFace*`）逐行移植；方塊、原木、石英柱、活塞底座與活塞頭已改走這條路
- [x] 紅石線、拉桿改為 `RenderBlocks` 逐行移植
- [ ] 其餘 `RenderBlocks` 渲染型態（火把、中繼器、按鈕、壓力板、樓梯、柵欄、門、液體、植物……）改成逐行移植
- [x] 伺服器開啟的介面（工作台、熔爐、箱子、死亡畫面）會釋放滑鼠
- [ ] 粒子系統（`EffectRenderer` 與所有 `EntityFX`）逐類移植
- [ ] `Entity` 基底：生物與玩家改用 `entity/base.rs`（水流推動、方塊碰撞反應、火）
- [ ] 實體依區塊分組（`Chunk.entityLists`），影響 `getEntitiesWithinAABB` 的順序
- [ ] 方塊屬性表（硬度、爆炸抗性）由 `Block.registerBlocks` 生成
- [ ] 隨機 tick 與排程 tick 統一為 `Block.updateTick`（樹苗、作物、耕地、莖、地獄疣、可可、熔岩點火……）

方塊破壞粒子以外的全部粒子類型核對、`RenderGlobal` 的雲層 fancy 模式、半透明在區塊內排序、物品附魔光澤、
生物裝備渲染、名牌、影子、`EntityRenderer` 的視角晃動與受傷傾斜、第三人稱視角、
GUI：信標、漏斗、投擲器（等方塊）、馬、村民交易、書、成就與統計、資源包、使用統計
