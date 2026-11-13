# MINERUST 接手手冊（給下一位開發者／AI）

這份文件每次推送都會一起更新。接手前請從頭讀完；它記錄專案目標、硬性準則、架構、
驗證方法、開發流程、目前進度、尚未完成的工作與已知問題。

- 進度與缺口的細目清單：[PARITY_TODO.md](PARITY_TODO.md)
- 原版 bug 的處理紀錄：[BUG_AUDIT.md](BUG_AUDIT.md)
- 使用方式與已驗證項目：[../README.md](../README.md)
- 開發日誌（每次推送追加一段）：本文件最後一節

---

## 1. 目標

把 Minecraft Java 版 1.7.10 **完整**以純 Rust 重寫，渲染使用 Vulkan。

- 所有行為、數值、亂數順序、視覺效果、介面、音效都與原版一致，100%。
- 對照的是 MCP 反編譯的原版原始碼，逐類別、逐函式移植，**不是「看起來像」**。
- 材質、聲音、語言檔不隨專案散布，執行時從使用者自己的 `1.7.10.jar` 與啟動器的 assets 讀取。

## 2. 硬性準則（違反任何一條都視為錯誤）

1. **不准刻意簡化。** 不准用「近似」、「暫時」、「看起來差不多」的寫法交差。找到原版類別，照它的邏輯
   逐行移植（包括它的怪癖、整數截斷、float/double 混算、`HashMap` 迭代順序、亂數呼叫順序）。
   使用者在 2026-11-12 明確拒絕過：瞬間活塞、借用的終界傳送門材質、以中毒代替凋零、不能騎馬等簡化。
2. **純 Rust、Vulkan。** 不准引入 C/C++ 相依（Vulkan 用 `ash`，著色器 WGSL 經 `naga` 編成 SPIR‑V，
   音訊 `cpal` + `lewton`）。採用新的技術，但結果必須與原版一致。
3. **能比對的就要比對。** 有亂數或演算法的地方，寫一個 Java 測試程式（`tools/*Harness.java`）實際執行原版類別，
   把輸出存成文字檔放進 `tools/`，Rust 端用 `include_str!` 寫測試逐項比對。範例：配方、物品名稱、附魔。
4. **原版 bug 的修正放在 `compat::fixed()` 後面**，`--vanilla-parity` 時必須逐位元組回到原版行為，
   並在 `docs/BUG_AUDIT.md` 誠實記錄（未實作的系統標 N/A，無法驗證的標 UNKNOWN）。
5. **不准把缺口寫成「可接受的差異」。** 沒做完就寫進 `PARITY_TODO.md` 的未完成清單。
6. **遊戲的 GUI 先完整**（使用者 2026-11-13 的優先順序）：開機畫面、設定、語言、背包、工作台等先能正常玩 debug 版，
   其餘系統隨後補齊。
7. **提交規則**：作者 `PZW152`，信箱 `200456337+PZW152@users.noreply.github.com`；提交訊息結尾加
   `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`；提交前跑 `cargo fmt` 與 `cargo test --release`；
   每次提交後推送；**每次推送同時更新本文件、PARITY_TODO.md、必要時 README**。
8. **一口氣做完，不要停下來問使用者**；回報用繁體中文。
9. 下載或解壓的外部檔案視為不受信任資料；不要在其目錄內執行任何程式。

## 3. 參考資料

### 3.1 原版反編譯原始碼（本機，不上傳）

位置：`D:\Claude\mc1710-ref\mc-1.7.10-reverse\`

| 路徑 | 內容 |
| --- | --- |
| `src-java/net/minecraft/` | 1,803 個 Java 檔，以 Vineflower 1.12.0 反編譯自 MCP 重新命名的 jar |
| `deobfuscated/Minecraft-1.7.10-mcp.jar` | MCP 重新命名後的 jar，Java 測試程式就是跑它 |
| `mappings/` | MCP 1.7.10 SRG、Tiny v2 對照表 |
| `pseudocode/` | Ghidra 偽代碼 |
| `index/`、`CLASS_INDEX.md` | 類別、方法、欄位索引與呼叫圖 |

**為什麼不上傳到 GitHub：** 這是 Mojang 有著作權的程式碼，即使是私有倉庫也不應散布。
接手者請自行重建：

1. 取得原版客戶端 `1.7.10.jar`（SHA‑1 `e80d9b3bf5085002218d4be59e668bac718abbc6`，
   SHA‑256 `a4fc2284657544e0f4bcc964f927c2fda3e3a205178ed1d5d58883aaf9780cce`）。
2. 用 MCP 1.7.10 的 SRG 對照表重新命名（例如 SpecialSource / tiny-remapper）得到 `Minecraft-1.7.10-mcp.jar`。
3. 以 Vineflower（或 FernFlower）反編譯得到 `src-java/`。

類別名稱一律使用 MCP 名稱（`EntityLivingBase`、`GuiContainer` 等），本專案的註解也用同樣的名稱標示出處，
例如 `/// GuiContainer.mouseClicked`，搜尋原始碼時直接用。

### 3.2 Java 測試程式的 classpath

`Minecraft-1.7.10-mcp.jar`、`guava-17.0.jar`、`gson-2.2.4.jar`、`commons-lang3-3.3.2.jar`、`commons-io-2.4.jar`、
`log4j-api-2.0-beta9.jar`、`log4j-core-2.0-beta9.jar`、`netty-all-4.0.10.Final.jar`、`authlib-1.5.21.jar`
（都在啟動器的 `libraries/` 裡）。JDK 21 可用。Windows 上 javac/java 的路徑要用 Windows 格式（`cygpath -w`）。
在 Java 端先呼叫 `Bootstrap.register()` 初始化方塊與物品登錄。

`tools/classpath.sh` 會組出這串 classpath（環境變數 `MC_LIBS` 指向啟動器的 `libraries`，`MCP_JAR` 指向重新命名的 jar）：

```
CP=$(sh tools/classpath.sh)
javac -cp "$CP" -d out tools/XxxHarness.java
java -cp "out;$CP" XxxHarness tools/xxx_1710.txt
```

### 3.3 遊戲資源

- `MC1710_JAR` 環境變數或 `--jar` 指向原版 jar（材質、`en_US.lang`、字型）。
- 啟動器的 assets（`assets/indexes/1.7.10.json` + `objects/`）提供其他語言檔與聲音。本機 jar：
  `C:\Users\PZW152\AppData\Roaming\Sapling\versions\1.7.10\1.7.10.jar`（SHA‑1 同上；舊路徑
  `Downloads\Sapling Data` 已被刪除，本機目前沒有完整的 assets 目錄，所以非英文語言與聲音無法在此機測試）。
- Git Bash 會把以 `/` 開頭的參數改寫成 Windows 路徑：送聊天指令（`MINERUST_CHAT="/xp 30L"`）時加
  `MSYS_NO_PATHCONV=1`。

## 4. 架構

| 模組 | 內容 |
| --- | --- |
| `src/sim.rs` | 伺服器執行緒（`Sim`），固定 20 tick；`Command`（客戶端→伺服器）與 `Event`（伺服器→客戶端）。子模組 `sim_vehicles.rs`、`commands.rs` 以 `#[path]` 掛在下面以存取私有欄位 |
| `src/play.rs` | 伺服器端的玩家狀態：物品欄、開著的視窗（`Open`）、方塊實體（`Tile`）、掉落物、方塊放置與使用 |
| `src/game.rs` | 客戶端（`Game`）：世界快照、輸入、相機、所有繪製的組合；子模組 `game_vehicles.rs` |
| `src/front.rs`、`src/menu/` | 沒有世界時的前端（標題畫面、選單）；`menu/mod.rs` 是各個 `GuiScreen` 的移植 |
| `src/container.rs` | `Container.slotClick`（七種模式）、`mergeItemStack`、各容器的 `transferStackInSlot`；伺服器與客戶端預測共用 |
| `src/gui_container.rs` | `GuiContainer` 的輸入與繪製，原版槽位編號 |
| `src/creative.rs` | `GuiContainerCreative` |
| `src/chat.rs`、`src/commands.rs` | `GuiNewChat` / `GuiChat` 與伺服器指令 |
| `src/crafting/` | `CraftingManager`（`data.rs` 由 `tools/RecipeHarness.java` 產生） |
| `src/item_data.rs`、`src/item_text.rs` | 物品登錄資料（`tools/ItemHarness.java` → `tools/gen_items.py` 產生）、名稱與工具提示 |
| `src/enchant.rs` | 附魔資料與 `EnchantmentHelper` |
| `src/inventory.rs` | `InventoryPlayer` |
| `src/repair.rs` | `ContainerRepair.updateRepairOutput`（純函式；名稱比較用共用語言表 `lang::shared()`，等同原版共用的 `StringTranslate`） |
| `src/render/item_block.rs` | `RenderBlocks.renderBlockAsItem`：方塊物品的面與前置轉換（GUI、掉落物、手持共用） |
| `src/gui_repair.rs` | `GuiRepair`：名稱欄、費用文字、`anvil.png` 的疊加部分 |
| `src/item_tag.rs` | 物品的 NBT（`stackTagCompound`）：以內容去重的全域表，`Stack.tag` 只是編號，所以 `Stack` 仍是 `Copy`；比較編號就是 `areItemStackTagsEqual`（與鍵的順序無關）。附魔、自訂名稱、`RepairCost`、染色、Lore 都在這裡；存檔寫進 Anvil 的 `tag` 與自有格式的 `t<hex>` |
| `src/entity/` | 實體：生物 AI、`EntityTrackerEntry`（`tracker.rs`）、船、礦車、箭、投擲物、經驗球、玩家物理 |
| `src/world/` | 世界生成、裝飾、結構、光照、紅石、液體、火、Anvil 存檔、爆炸 |
| `src/render/` | Vulkan（`vk.rs`）、方塊網格化（`mesher.rs`、`rb.rs` 是 `RenderBlocks` 的機械式移植）、模型（`model*.rs`）、箱子、告示牌、圖集 |
| `src/gui.rs`、`src/screens.rs` | `FontRenderer` 移植、HUD、按鈕、通用繪圖 |
| `src/settings.rs`、`src/lang.rs`、`src/assets.rs` | `GameSettings` / `options.txt`、語言檔、啟動器資源索引 |

重點設計：
- 客戶端只有伺服器送來的快照；容器點擊在客戶端用同一份 `container.rs` 預測，伺服器的 `clicks` 計數讓客戶端丟掉過時的快照。
- 生物在客戶端有自己的副本：伺服器端 `Mob::track()` 模擬 `EntityTrackerEntry`（1/32 座標、4/32 門檻、S19 頭部），
  客戶端以三步內插（`setPositionAndRotation2`）、`updateDistance` 與肢體擺動自行計算。
- GUI 圖集 `UI_SIZE` 2048；方塊／實體圖集 `ATLAS_SIZE` 2048；UI 管線沒有深度測試，GUI 中的 3D 模型要先排序
  （`model::gui_quads` + `sort_quads`）。

## 5. 開發與驗證流程

```
cargo fmt
cargo test --release
MC1710_JAR="C:/…/1.7.10.jar" cargo test --release      # 需要 jar 的測試（物品英文名稱全部比對）
```

### 5.1 截圖驗證（不用手動操作）

```
minerust --seed 12345 --no-save --size 854 480 --screenshot out.png 6
```

可搭配的除錯參數與環境變數：

| 用途 | 方法 |
| --- | --- |
| 標題畫面、指定選單 | `--title`、`--menu options,video,chat,...` |
| 開背包／創造模式分頁 | `--open inventory`、`--open creative:<0..11>`（需 `--creative`） |
| 右鍵某方塊（開工作台、箱子、放告示牌） | `--open use:x,y,z`，搭配 `MINERUST_SETBLOCK="x y z id meta;..."` |
| 第三人稱 | `--open view:1` 或 `view:2` |
| 聊天 | `--open chat`；`MINERUST_CHAT="/give Player stone;..."` 開局送出 |
| 視窗點擊腳本 | `MINERUST_CLICKS="slot:button:mode,..."` |
| 固定滑鼠位置 | `MINERUST_MOUSE="x,y"`（GUI 座標） |
| 給物品、效果、召喚 | `--give id:meta:count,...`、`--effect id:秒:等級`、`--summon Cow,Zombie` |
| 告示牌文字 | `MINERUST_SIGN="a|b|c|d"`，`MINERUST_SIGN_DONE=1` 直接關閉編輯器 |
| 不等區塊載完就截圖 | `MINERUST_NOWAIT=1` |
| 自動建立新世界 | `MINERUST_AUTOCREATE=<名稱>` |

### 5.2 與原版逐位元組比對

| 項目 | 指令 |
| --- | --- |
| 地形 | `minerust --dump <seed> <n> rust.bin`；`java Harness <seed> <n> java.bin`；`minerust --compare rust.bin java.bin` |
| 裝飾與光照 | `--dump-pop` / `PopHarness` / `--compare-pop` |
| Anvil 區域檔 | `tools/AnvilHarness.java` 雙向讀寫 |
| 配方 | `tools/RecipeHarness.java` → `tools/recipes_1710.txt` → `src/crafting/data.rs` |
| 物品、創造分頁、登錄名稱、生怪蛋顏色 | `tools/ItemHarness.java` → `tools/items_1710.txt` → `python tools/gen_items.py` |
| 附魔清單與附魔台等級 | `tools/EnchantHarness.java` → `tools/enchant_1710.txt`（測試 `lists_and_levels_match_the_game`） |
| 皮甲染色配方 | `tools/DyeHarness.java` → `tools/dyes_1710.txt`（測試 `armor_dyes_match_the_game`） |
| 鐵砧的修理／合併／命名費用 | `tools/RepairHarness.java` 讀 `tools/repair_cases.txt` → `tools/repair_1710.txt`（測試 `anvil_matches_the_game`） |

新增系統時照同樣模式：寫 Harness → 存文字檔 → Rust 測試比對。

## 6. 進度（截至本次推送）

已完成且有驗證的大項見 README 的「已移植」與 PARITY_TODO 的勾選項。近期完成：

- 標題畫面（全景圖）、選項、影像、音效、按鍵、語言、聊天設定、建立／選擇世界、暫停選單、載入畫面
- 背包、工作台、熔爐、釀造台、箱子、礦車箱子、發射器：`Container.slotClick` 全模式、配方 317 條
- 創造模式物品欄（分頁、搜尋、捲動、垃圾桶）
- 聊天與 18 個指令、F5 第三人稱、F1、按鍵設定
- 盔甲（穿戴、模型、圖示）、手持物品、玩家模型
- 箱子模型與開蓋、告示牌（放置、編輯、文字、存檔）
- 附魔資料與 `EnchantmentHelper`（與原版 385 筆比對一致）
- 附魔台：方塊、`ContainerEnchantment`（書架計數、三個等級、附魔與扣等級、關閉時掉出）、`GuiEnchantment`
  （書本模型在自己的透視視窗中、銀河字母字型與換行、三種按鈕狀態）、浮空書本的動畫與繪製
- 鐵砧：方塊（`renderBlockAnvil` 四個方塊與 uvRotate、損壞頂面、朝向、碰撞箱、掉落、下落與落下實體的繪製、
  物品圖示的 3D 模型）、`ContainerRepair`（`updateRepairOutput` 與原版 25 組比對一致、`canTakeStack`、
  扣等級、12% 損壞與音效、關閉時掉出）、`GuiRepair`（名稱欄、費用文字與三重陰影、紅叉）、`MC|ItemName`
- 方塊物品的 3D 模型：`render/item_block.rs` 移植 `renderBlockAsItem`（`renderItemIn3d` 接受的 render type 0/10/11/13/16/26/27/31/35/39，
  含 `setBlockBoundsForItemRender` 的尺寸），物品欄以 `enableGUIStandardItemLighting` 打光，掉落物與手持物品共用同一份模型

## 7. 尚未完成（依優先順序）

1. **附魔效果本身**：附魔台已完成，但多數附魔尚未作用在遊戲上（鋒利／不死剋星／節肢剋星、擊退、燃燒、掠奪、
   效率、絲綢之觸、耐久、幸運、保護系列、水下呼吸、親水性、荊棘、力量／衝擊／火焰／無限、海之眷顧、魚餌）。
   附魔台的書架粒子（`EntityEnchantmentTableParticleFX`）也還沒做。
2. （已完成）方塊物品的 3D 模型，見第 6 節。
3. 其他缺方塊的 GUI：信標、漏斗、投擲器、馬的物品欄、村民交易、書與羽毛筆。
4. 統計與成就（`StatList`、`AchievementList`、`GuiStats`、`GuiAchievements`），資源包畫面、使用統計畫面。
5. 物品 NBT 已有（見第 4 節），還缺用到它的東西：煙火、書與羽毛筆、地圖資料，以及三個特殊配方（煙火、書本複製、地圖擴大）；附魔光澤。
6. 缺少的方塊（見 PARITY_TODO B 節）、比較器、漏斗、投擲器、觸發鐵軌等。
7. 告示牌上的非 ASCII 文字（原版用 unicode 字頁，目前只畫 ASCII）。
8. 指令：summon、enchant、gamerule、playsound、spreadplayers、testfor 等與 Tab 補完。
9. 生物裝備、名牌、陰影、受傷傾斜、多人連線。
10. 已知的光照差異：3 個巨型叢林區樹冠、1 座沙漠神殿內部天空光（原因未查出）。

## 8. 已知注意事項

- 本機沒有 Vulkan validation layer；`--validate` 無法在此機驗證。
- 舊格式存檔（`level.txt`）不會出現在原版的世界選單，這是原版行為。
- 平坦、巨型群系、放大化等世界類型尚未改變生成；獎勵箱、極限模式尚未生效。
- 聊天打字與創造搜尋用 `text_input()` 走 `Game::text_key`，按鍵事件需要字元時記得接上。
- Bash heredoc 內含撇號會壞；修補腳本請寫成檔案再執行。

## 9. 開發日誌

每次推送在這裡追加一段：日期、提交、做了什麼、怎麼驗證、留下什麼。

### 2026-11-13
- `b51da64` Container / 配方 / 盔甲欄 / 工具提示：`slotClick` 移植並以客戶端預測共用；317 條配方逐條取自原版；554 個物品英文名稱與原版一致。
- `9991930` 修標題全景圖旋轉 90°（最後一個 quad 取的是第 6 次模糊後的材質）、生物身體不轉向與抽搐（補 `updateDistance`、`EntityTrackerEntry` 與客戶端內插）、草葉碎屑顏色（`getIcon(0)` + `colorMultiplier`）。
- `ce8477f` 創造模式物品欄、箱子模型。
- `19c00c4` 聊天與指令、F5、按鍵設定。
- `d89b492` 盔甲與手持物品繪製、穿戴盔甲、南瓜視野。
- `0f958b9` 聊天設定、載入畫面、發射器介面。
- `893a6c1` 告示牌全套。
- `5de6c71` 附魔資料與 `EnchantmentHelper`（`tools/EnchantHarness.java` 385 筆一致）、附魔台方塊、這份接手手冊。
- `cc422c5` 附魔台全套（容器、介面、浮空書本）；測試 `enchanting_table_enchants_and_costs_levels`。
- `3239bf6` 物品 NBT（`item_tag.rs`），附魔改存在 NBT 裡（Anvil 存檔以前會遺失附魔，現在不會）；自訂名稱、染色、Lore 的工具提示；皮甲染色顏色用在圖示與盔甲模型；`RecipesArmorDyes`（與原版 9 組一致）；`tools/classpath.sh`（舊的 `Sapling Data` 函式庫已刪除）。
- 本次：鐵砧全套。方塊與繪製（世界、落下實體用 `MovingDraw::Anvil` 取世界光照、物品圖示）、`repair.rs`（與原版 25 組一致）、容器的 `canTakeStack` 檢查補進 `slotClick` 各模式、`GuiRepair`、`Command::RenameItem`；`lang::shared()`；測試 `anvil_repairs_renames_and_costs_levels`；截圖確認介面、圖示、損壞頂面。
- 本次：方塊物品 3D 模型（樓梯、柵欄、仙人掌、活塞、壓力板、按鈕、地板門、地毯、雪片、半磚、附魔台、終界門框、龍蛋、鐵砧），取代舊的簡化正方體與平面貼圖；GUI 打光改為原版光源方向；截圖確認創造模式三個分頁與手持樓梯。
