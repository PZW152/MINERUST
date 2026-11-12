# 與原版 1.7.10 的差距清單

原則：照 `src-java/net/minecraft/` 的原始類別逐函式移植，不做任何「近似」或「簡化」。
做完一項就從這裡刪掉並寫進 README；這份清單只列還沒做到 100% 的部分。

## A. 先前被當成「刻意差異」的簡化（優先）

- [x] 活塞：`BlockPistonMoving` + `TileEntityPiston`（兩 tick 的移動、推動實體、`TileEntityRendererPiston` 動畫）、
      `World.addBlockEvent` 佇列
- [ ] 終界傳送門：`TileEntityEndPortalRenderer`（16 層星空視差）
- [ ] 凋零效果與整套 `Potion` / `PotionEffect`（現在以中毒代替）
- [ ] 騎乘：`Entity.mountEntity`、`updateRidden`，馬（跳躍、鞍、馬鎧、箱子、馬的物品欄）、豬＋鞍＋胡蘿蔔釣竿
- [ ] 傳送門建造時的中繼值與原版一致
- [ ] 實體（生物、掉落物、箭、礦車）穿越傳送門 `Entity.travelToDimension`
- [ ] 落沙／礫石／鐵砧／龍蛋的 `EntityFallingBlock`
- [ ] 雙層植物上半燒掉時的掉落行為放到 `--vanilla-parity` 開關後
- [x] 掉落物：`EntityItem` 逐行重寫（`pushOutOfBlocks`、水流、岩漿燃燒、合併、撿拾音效與飛向玩家的動畫）
- [ ] 箭的命中判定簡化（`EntityArrow.onUpdate` 的 `rayTrace` + 實體 AABB 射線）
- [ ] 爆炸的 `getBlockDensity` 當成無遮蔽
- [ ] GUI 漸層用兩個矩形近似（`drawGradientRect`）

## B. 缺少的方塊（28 種）

床、移動中活塞(36)、告示牌(63/68)、蛋糕、染色玻璃／玻璃片、南瓜梗／西瓜梗、柵欄門、附魔台、釀造台、木半磚、
終界箱、指令方塊、信標、圓石牆、頭顱、鐵砧、陷阱箱、測重壓力板、比較器、陽光感測器、漏斗、石英樓梯、
觸發鐵軌、投擲器

另外：可可豆（目前不繪製）、箱子模型與開蓋動畫（`TileEntityChestRenderer`，目前是方塊）

## C. 物品行為

盔甲（穿戴、護甲值、耐久、渲染）、食物（全部）、藥水／噴濺藥水／玻璃瓶、桶（水、岩漿、牛奶）、骨粉、鋤頭、剪刀、
釣竿、終界珍珠、終界之眼、雪球、蛋、經驗瓶、火焰彈、煙火、地圖、指南針、時鐘、船、礦車（五種）、鞍、拴繩、
命名牌、生怪蛋、書與筆／完成的書、唱片與唱片機、畫、物品展示框、床、告示牌、蛋糕、門、紅石中繼器／比較器物品

## D. 實體

船、礦車（一般、箱子、熔爐、TNT、漏斗、生怪磚、指令方塊）、畫、物品展示框、拴繩結、終界珍珠、終界之眼、
浮標、煙火、經驗球、噴濺藥水、凋零怪與凋零頭、閃電、雪人、巨人、殭屍村民、小殭屍、雞騎士、蜘蛛騎士、
生物穿戴裝備與撿拾物品、村民交易與繁殖、動物繁殖與幼年

## E. 系統

附魔（附魔台、所有附魔效果）、鐵砧（修理、命名）、釀造、信標、成就與統計、指令與聊天、遊戲模式（創造、冒險、
極限）、難度、世界建立選單、選項選單、資源包、語言、地圖資料、終界詩與製作人員名單、多人與區域網路（協定 5）

## F. 渲染

- [x] `RenderBlocks.renderStandardBlock`（含兩種平滑光照與六個 `renderFace*`）逐行移植；方塊、原木、石英柱、活塞底座與活塞頭已改走這條路
- [ ] 其餘 `RenderBlocks` 渲染型態（樓梯、柵欄、門、液體、植物、紅石線……）改成逐行移植
- [ ] 粒子系統（`EffectRenderer` 與所有 `EntityFX`）逐類移植
- [ ] `Entity` 基底：生物與玩家改用 `entity/base.rs`（水流推動、方塊碰撞反應、火）
- [ ] 實體依區塊分組（`Chunk.entityLists`），影響 `getEntitiesWithinAABB` 的順序
- [ ] 方塊屬性表（硬度、爆炸抗性）由 `Block.registerBlocks` 生成
- [ ] 隨機 tick 與排程 tick 統一為 `Block.updateTick`（樹苗、作物、耕地、莖、地獄疣、可可、熔岩點火……）

方塊破壞粒子以外的全部粒子類型核對、`RenderGlobal` 的雲層 fancy 模式、半透明在區塊內排序、物品附魔光澤、
生物裝備渲染、名牌、影子、`EntityRenderer` 的視角晃動與受傷傾斜、第三人稱視角、GUI 全部畫面
