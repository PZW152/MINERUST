# 與原版 1.7.10 的差距清單

這是專案**唯一**的「還沒做完」清單。README 只寫概況，HANDOFF 只寫怎麼做，進度一律記在這裡。

規則：

- 只列還沒做到與原版一致的部分。做完就刪掉那一行（做了什麼記在 [CHANGELOG.md](CHANGELOG.md) 與提交訊息）。
- 不准把缺口寫成「可接受的差異」。照 `src-java/net/minecraft/` 的原始類別逐函式移植，不做近似或簡化。
- 標「需確認」的是整理時沒有逐項核對過的舊項目：先查程式碼，做過了就刪掉，沒做就去掉「需確認」。

## 1. 下一步（依優先順序）

1. **方塊外觀改為 `RenderBlocks` 逐行移植**（第 5 節）。目前還在 `mesher.rs` 的 `shaped()` 裡手寫近似的渲染型態很多，
   這是畫面與原版不同的主要來源。
2. **影像設定還沒生效的項目**（第 5 節）。選單能調但畫面不變，算行為差異。
3. **生物 AI 第二階段**（第 2 節）：馬、村民、終界使者等還在手寫的處理函式上。
4. **生物的其餘行為**（第 2 節）：雞騎士、增援、破門、遊戲規則 `mobGriefing` 等。
5. **缺少的方塊、物品、實體與介面**（第 3、4 節）。

## 2. 生物

AI：
- [ ] 馬：還借用舊的 `animal()`。需要 `EntityAIRunAroundLikeCrazy`、`Panic`、`Mate`、`FollowParent`、`Wander`、`WatchClosest`、`LookIdle`
- [ ] 村民：交易、開門、回屋、繁殖、跟隨鐵魔像、`setLookingForHome`（都需要村莊資料 `VillageCollection`）
- [ ] 鐵魔像：`DefendVillage`、`MoveThroughVillage`、`MoveTowardsRestriction`（需要村莊資料）；拿花（`setHoldingRose` 只有計時，沒有畫）
- [ ] 狼的乞食歪頭與甩水動畫、被非玩家打時傷害減半；豹貓 `OcelotSit` 對床與「箱子有人在用」的判斷
- [ ] 騎豬（`EntityAIControlledByPlayer`）
- [ ] 終界使者、烈焰使者、惡魂、蝙蝠、魷魚的 `updateEntityActionState`（`extra.rs`、`nether.rs`）逐行對照原始碼
- [ ] 蠹魚鑽進石頭與召喚同伴（`EntitySilverfish.updateEntityActionState`）
- [ ] `EntityAITarget` 的 `nearbyOnly`（`canEasilyReach`）與家的範圍（`isWithinHomeDistance`）

生成與狀態：
- [ ] 雞騎士、蜘蛛騎士（亂數照抽但不生成）
- [ ] 殭屍增援（`summonReinforcements`、領頭殭屍的增援機率）與破門（`EntityAIBreakDoor`）
- [ ] `onSpawnWithEgg` 的其餘部分：殭屍的擊退抗性加成、跟隨距離的 `nextGaussian`、蜘蛛在困難模式的藥水效果、
      豹貓生成時的幼貓、馬的逐行核對
- [ ] 區塊的 `InhabitedTime` 寫進存檔（目前寫 0，重新載入從 0 開始）
- [ ] 高壓苦力怕（閃電劈中，爆炸半徑 6）
- [ ] 遊戲規則 `mobGriefing`（目前一律視為開啟）與其他遊戲規則

數值與外觀：
- [ ] 藥水效果尚缺：隱形時不畫出生物、生物盔甲上的保護附魔（`applyArmorCalculations` 之後的 `getEnchantmentModifierDamage`）、
      吸收與生命提升、凋零骷髏與洞穴蜘蛛打其他生物時的效果
- [ ] `getEyeHeight` 的各職業覆寫（狼 `height × 0.8` 等）
- [ ] 狼、豹貓、馬的幼體模型（`isChild` 的畫法）
- [ ] 受傷時的 `limbSwingAmount = 1.5`；受傷鏡頭傾斜的方向（`attackedAtYaw`，目前只有傾斜沒有方向）
- [ ] 生物裝備耗損到壞掉時保留數量 0 的物品堆（原版行為）
- [ ] 實體依區塊分組（`Chunk.entityLists`），影響 `getEntitiesWithinAABB` 的順序與推擠的順序
- [ ] 實體（生物、掉落物、箭、礦車）穿越傳送門（`Entity.travelToDimension`），目前只有玩家會傳送
- [ ] 實體陰影（`Render.renderShadow`）、名牌（`renderLivingLabel`）

## 3. 方塊與世界

缺少的方塊：
- [ ] 染色玻璃片、終界箱、指令方塊、信標、頭顱、陷阱箱、測重壓力板、比較器、陽光感測器、漏斗、觸發鐵軌、投擲器、唱片機（需確認）
- [ ] 可可豆的繪製（目前會長，但不畫）
- [ ] 睡覺時玩家模型躺在床上的畫法（第三人稱）

放置與掉落：
- [ ] 客戶端預測：有朝向或需依附的方塊（原木、火把、樓梯、半磚、門……，要把 `play.rs` 決定中繼值的邏輯抽成兩端共用）、
      沙與礫石、TNT、雙層植物的另一半
- [ ] 放置規則：頭顱、可可、終界門框的朝向，以及其餘需要 `canPlaceBlockOnSide` 檢查的方塊（需確認）
- [ ] 掉落的其餘路徑改走 `drops.rs`：`harvestBlock` 的覆寫（剪刀剪樹葉／草／藤蔓、冰、雪片，`play.rs` 約 737、752 行）
- [ ] `item::has_subtypes` 改用原版 `getHasSubtypes` 的表（目前用 `max_damage == 0` 近似）
- [ ] 方塊破壞時容器內容物的散落方式（需確認）
- [ ] 雙層植物上半燒掉時的掉落行為放到 `--vanilla-parity` 開關後（BUG_AUDIT MC-29512）
- [ ] 方塊屬性表（硬度、爆炸抗性）由 `Block.registerBlocks` 生成

世界：
- [ ] 世界亂數借出期間的間接使用會讓亂數順序與原版分岔（見 HANDOFF 第 7 節）
- [ ] 傳送門建造時的中繼值與原版一致（目前把軸向存在中繼值裡）
- [ ] 平坦、巨型群系、放大化等世界類型的生成；獎勵箱；極限模式
- [ ] 難度存進 `level.dat` 的 `Difficulty`
- [ ] 存檔只寫有變動或有實體的區塊（`Chunk.needsSaving`），目前讀進來過的區塊每次都重寫
- [ ] 已知的光照差異：3 個叢林區巨樹樹冠附近的天空光、1 座沙漠神殿塔內的天空光（原因未查出，只在 `--vanilla-parity` 看得到）

## 4. 物品、實體、介面、系統

物品：
- [ ] 釣竿與釣魚（連帶海之眷顧、魚餌、釣魚經驗）
- [ ] 玻璃瓶裝水（`ItemGlassBottle.onItemRightClick`，需確認）
- [ ] 生怪蛋、終界之眼（丟出）、火焰彈、煙火、地圖、指南針與時鐘、鞍、拴繩、命名牌、書與羽毛筆、唱片、畫、物品展示框（需確認哪些已有）
- [ ] 特殊配方：煙火、書本複製、地圖擴大（配方資料裡有，結果需確認）
- [ ] 物品名稱與說明一律由 `en_US.lang` 取得（藥水前綴與後綴、附魔書等）

實體：
- [ ] 畫、物品展示框、拴繩結、終界之眼、浮標、煙火、凋零怪與凋零頭、閃電實體、雪人、巨人
- [ ] 騎乘：馬（跳躍、鞍、馬鎧、箱子、物品欄）、豬＋鞍＋胡蘿蔔釣竿；騎乘時玩家模型的坐姿（`ModelBiped.isRiding`）
- [ ] 礦車：漏斗礦車的吸取、指令方塊與生怪磚礦車、觸發鐵軌、`MovingSoundMinecart` 的連續音效、騎乘中生物的存檔（`Riding`）
- [ ] 掉落物改為客戶端自行模擬（原版客戶端也跑 `EntityItem.onUpdate`，伺服器每 20 tick 校正）
- [ ] 經驗來源：繁殖、釣魚、村民交易

介面：
- [ ] 信標、漏斗、投擲器、馬的物品欄、村民交易、書與羽毛筆
- [ ] 統計與成就：已有統計檔（`stats/<uuid>.json`，`src/stats.rs`）、成就樹、成就提示框（`GuiAchievement`）與
      「Taking Inventory」。待做：`StatList` 的一般統計、其餘 32 個成就的觸發點、`GuiStats`、`GuiAchievements`、
      `/achievement` 指令、成就的聊天公告
- [ ] 資源包、使用統計（Snooper）、直播畫面
- [ ] 告示牌上的非 ASCII 文字（原版用 unicode 字頁）
- [ ] 指令：summon、enchant、gamerule、playsound、spreadplayers、testfor 等，Tab 自動補完

系統：
- [ ] 多人與區域網路（協定 5）
- [ ] 附魔台的書架粒子（`EntityEnchantmentTableParticleFX`）
- [ ] 破損物品的碎片粒子（客戶端）

## 5. 渲染

- [ ] `RenderBlocks` 其餘渲染型態改為逐行移植（已完成：標準方塊、原木、石英柱、活塞、釀造台、鐵砧、紅石線、拉桿、
      火把、中繼器（含鎖住時的基岩條）、梯子、鐵軌、作物、仙人掌，以及走 `renderStandardBlock` 的半磚、雪層、
      按鈕、壓力板、地毯、活板門、耕地、附魔台）。
      待做：比較器（方塊本身還沒有）、絆線鉤與絆線、門、柵欄、玻璃板、樓梯、
      液體、莖、藤蔓、睡蓮、可可、火、床、煉藥鍋、終界門框、龍蛋、花盆等
- [ ] 影像設定尚未生效：畫質「流暢」時樹葉的物品圖示與水面平滑著色（`EntityRenderer` 1055 行）、各向異性過濾、
      3D 立體（anaglyph）、進階 OpenGL（遮蔽查詢）、FBO；平滑光照關閉時，`mesher.rs` 裡還沒改走 `rb` 的方塊（樓梯、柵欄、液體等）不會跟著變
- [ ] 世界中的附魔光澤（手持、第一人稱、掉落物、盔甲模型）、方塊物品的光澤；手持箱子與掉落箱子的模型
- [ ] 粒子系統（`EffectRenderer` 與所有 `EntityFX`）逐類核對
- [ ] 第一人稱著火畫面目前畫在暈影之上（原版在暈影之下）
- [ ] 與原版截圖比對（`tools/shotcompare.py`）找到、還沒查原因的差異：
  - 手持物品（第一人稱）仍用固定的面亮度，原版用 `RenderHelper.enableStandardItemLighting`（空手的手臂已改，見 `entities::hand`）
  - F3 的 `C:` 行（`f3` 場景）：同一位置原版 649/4624、F 924、E 3051，這裡 515、F 970、E 3139。先對照
    `RenderGlobal.markRenderersForNewPosition` 的渲染區塊格位置（原版不是以相機所在區塊為中心），再看哪些區塊算空的
- [ ] 伺服器送給客戶端的範圍（F3 對照找到）：渲染距離 8 時原版客戶端有 289 個區塊（`PlayerManager`，17×17），這裡 441 個
      （`Sim` 的 `spiral(v + 2)`）；客戶端的實體 376 個對原版 100 個，應照 `EntityTracker` 各類別的追蹤距離送出
