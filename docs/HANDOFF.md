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
   `Co-Authored-By:`（寫實際使用的模型，例如 `Claude Fable 5.2 <noreply@anthropic.com>`）；提交前跑 `cargo fmt` 與 `cargo test --release`；
   每次提交後推送；**每次推送同時更新本文件、PARITY_TODO.md、必要時 README**。
8. **一口氣做完，不要停下來問使用者**；回報用繁體中文。
9. 下載或解壓的外部檔案視為不受信任資料；不要在其目錄內執行任何程式。
10. **效能是需求的一部分**（使用者 2026-11-13：「性能甚至比原版還要垃圾」）。動到渲染、網格化、區塊傳送、
    伺服器 tick 的改動，提交前跑第 5.3 節的基準測試，數字不能比日誌裡上一次記錄的差；變差就先查原因。

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
  `Downloads\Sapling Data` 已被刪除）。assets 就在同一個啟動器根目錄 `AppData\Roaming\Sapling\assets`，
  聲音與各語言都可在此機測試。
- 不帶參數啟動時 `find_jar` 依序找：`--jar`、`MC1710_JAR`、exe 旁、工作目錄、`%APPDATA%` 下的
  `.minecraft`／`Sapling`／`.hmcl`／`PCL`／`.tlauncher` 的 `versions\1.7.10\1.7.10.jar`；assets 取 jar 往上三層的 `assets`。
  AppData 位置改由 `SHGetKnownFolderPath(FOLDERID_RoamingAppData)` 取得，不依賴環境變數；都找不到時跳出檔案選擇視窗，選好的路徑存在 exe 旁的 `minerust_jar.txt`。仍失敗時錯誤對話框會列出找過的所有位置。截圖模式（`--screenshot`）刻意關閉聲音。
- **陷阱：Claude 桌面版是 MSIX 套件，從它啟動的程序看到的 `%APPDATA%` 是虛擬化的**。`AppData\Roaming\Sapling` 實際在
  `AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Sapling`，使用者自己雙擊 exe 時看不到。
  因此原版 jar 與 1.7.10 的 assets 複製到了 `D:\Claude\mc1710-rs\game\`（`versions\1.7.10\1.7.10.jar`、`assets\`，已加入 .gitignore，
  不推送）；`find_jar` 會在 exe 往上四層找 `game\`，也會掃 `%LOCALAPPDATA%\Packages\*\LocalCache\Roaming`。
  驗證「使用者能不能開」時，不能只在這裡跑：要確認啟動訊息 `Jar:` 印出的路徑不在 AppData 底下。
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
| `src/drops.rs` | 方塊掉落：`dropBlockAsItemWithChance`、`quantityDropped(WithBonus)`、`getItemDropped`、`damageDropped`、`canSilkHarvest`、`createStackedBlock`，逐類別移植，亂數順序含每個掉落物的三個位置浮點數；`ServerWorld::drop_block_with_chance` / `spawn_drops` 把結果生成到世界（玩家挖掘、爆炸、活塞共用）。`drops_silk.rs` 是由參考資料產生的兩張表 |
| `src/inventory.rs` | `InventoryPlayer` |
| `src/repair.rs` | `ContainerRepair.updateRepairOutput`（純函式；名稱比較用共用語言表 `lang::shared()`，等同原版共用的 `StringTranslate`） |
| `src/render/item_block.rs` | `RenderBlocks.renderBlockAsItem`：方塊物品的面與前置轉換（GUI、掉落物、手持共用） |
| `src/gui_repair.rs` | `GuiRepair`：名稱欄、費用文字、`anvil.png` 的疊加部分 |
| `src/item_tag.rs` | 物品的 NBT（`stackTagCompound`）：以內容去重的全域表，`Stack.tag` 只是編號，所以 `Stack` 仍是 `Copy`；比較編號就是 `areItemStackTagsEqual`（與鍵的順序無關）。附魔、自訂名稱、`RepairCost`、染色、Lore 都在這裡；存檔寫進 Anvil 的 `tag` 與自有格式的 `t<hex>` |
| `src/entity/` | 實體：生物 AI、`EntityTrackerEntry`（`tracker.rs`）、船、礦車、箭、投擲物、經驗球、玩家物理 |
| `src/world/` | 世界生成、裝飾、結構、光照、紅石、液體、火、Anvil 存檔、爆炸 |
| `src/render/` | Vulkan（`vk.rs`）、方塊網格化（`mesher.rs`、`rb.rs` 是 `RenderBlocks` 的機械式移植）、模型（`model*.rs`）、箱子、告示牌、圖集 |
| `src/render/vk.rs` 的區塊部分 | 以 16×16×16 的渲染區塊（原版 `WorldRenderer`）為單位：`GpuColumn.secs[16]`，頂點放在顯存池 `ArenaBlock`（每塊 20 MB、`DEVICE_LOCAL`），經每幀的暫存緩衝 `mesh_stage` 以 `cmd_copy_buffer` 上傳（`flush_uploads`），舊範圍延後 3 幀回收 |
| `src/gui.rs`、`src/screens.rs` | `FontRenderer` 移植、HUD、按鈕、通用繪圖 |
| `src/settings.rs`、`src/lang.rs`、`src/assets.rs` | `GameSettings` / `options.txt`、語言檔、啟動器資源索引 |

重點設計：
- **髒標記以 16³ 分段為單位。** 伺服端 `ServerWorld.dirty` 是 `區塊 → 16 位元遮罩`；`mark_dirty(x, y, z)` 標記該方塊與
  周圍 26 格所在的渲染區塊（等同原版 `markBlockForUpdate` → `RenderGlobal.markBlocksForUpdate(x-1 … x+1)`），
  `Event::Chunk.changed` 把遮罩帶給客戶端，客戶端 `Game.dirty` 只重建那些分段（`mesher::build(.., mask)`）。
  **不要再把整個區塊柱標髒**：改動前閒置世界每秒重建 2,200 個整柱網格，幀率只有 100。
- 一個區塊柱要等自己與八個鄰居都 `ready`（不會再被裝飾寫入）才第一次網格化，載入時每柱只建一次。
- 方塊實體清單（箱子、告示牌、附魔台、終界門）只重掃有變動的分段（`ClientChunk::rescan`）。
- 影像設定的生效點：`Game::apply_video_settings`（垂直同步、平滑光照、Mipmap 層級），幀率上限在 `main.rs` 的
  `FrameSync`（LWJGL `Sync` 的移植，260 = 不限制；沒有世界時固定 30），全螢幕（F11／設定）每幀與視窗狀態同步。
- **兩個時鐘。** 伺服器與客戶端各自以 20 Hz 走 tick，相位不同。規則：
  - 伺服器每個 tick 的事件以 `Event::Tick(完成時間)` 收尾，客戶端收齊整個 tick 才一次套用（`Game.inbox` → `arrived`）。
  - 由伺服器模擬、客戶端只負責畫的東西（掉落物、箭、TNT、火球、投擲物、落沙、活塞）用 `Game::server_partial()`
    （距離該 tick 完成的時間 ÷ 50 ms）在 `prev`→`pos` 之間插值，**不能用客戶端的 `partial`**，否則會前後跳動。
  - 客戶端自己跑 tick 的東西（玩家、生物的三步內插、經驗球、船與礦車）用客戶端的 `partial`。
  - 收到 `Event::Tick` 時把客戶端 tick 相位緩慢拉到伺服器 tick 之後半個 tick（`pump_chunks` 內），
    避免兩邊相位貼近時生物的三步內插拿到 2 或 4 個客戶端 tick 而一頓一頓。
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
| 方塊掉落（175 種方塊 × 16 中繼值 × 時運 0–3 × 4 個種子，共 4 萬多筆，含經驗球與亂數順序） | `tools/DropHarness.java` → `tools/drops_1710.txt`（測試 `drops_match_the_game`） |
| 皮甲染色配方 | `tools/DyeHarness.java` → `tools/dyes_1710.txt`（測試 `armor_dyes_match_the_game`） |
| 方塊貼圖（每個方塊 × 中繼值 × 面的 `Block.getIcon`） | `tools/IconHarness.java` → `tools/icons_1710.txt`（測試 `block_textures_match_the_game`；由專屬繪製函式自選貼圖的方塊列在測試的 `OWN` 裡） |
| 鐵砧的修理／合併／命名費用 | `tools/RepairHarness.java` 讀 `tools/repair_cases.txt` → `tools/repair_1710.txt`（測試 `anvil_matches_the_game`） |

新增系統時照同樣模式：寫 Harness → 存文字檔 → Rust 測試比對。

### 5.3 效能基準（動到渲染或區塊流程必跑）

```
MINERUST_BENCH=10 minerust --seed 12345 --no-save --size 1920 1080 --distance 16 --time 6000 --freeze-time
```

世界載完後關閉垂直同步與幀率上限、鏡頭緩慢旋轉，量 10 秒，印出：載入秒數、平均／中位數／99%／99.9%／最大幀時間、
每幀在「區塊泵送／tick／組幀／渲染」各花多少、伺服器 tick 平均與最差、區塊快照與分段上傳的總數。
`MINERUST_BENCH_VSYNC=1` 保留垂直同步，`MINERUST_BENCH_STILL=1` 鏡頭不轉。

基準（RTX 5070 Laptop、1920×1080、種子 12345）：

| | 視距 12 | 視距 16 |
| --- | --- | --- |
| 改動前（`e87f170`） | 106 fps，載入 4.6 秒 | 97 fps，載入 24.0 秒 |
| 分段網格 + 顯存池之後 | 1,575 fps（平均 0.64 ms，99% 2.4 ms），載入 3.9 秒 | 1,311 fps（平均 0.76 ms，99% 2.7 ms），載入 7.1 秒 |

伺服器 tick：視距 16 閒置平均 5.0 ms、最差 11.4 ms（預算 50 ms）。

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

0. **影像設定還沒生效的項目**（選單裡能調，但畫面不變，這是與原版的行為差異）：
   - 畫質「流暢」（`fancyGraphics = false`）：不透明樹葉（`leaves_*_opaque` 與 `isOpaqueCube`）、平面雲
     （`RenderGlobal.renderClouds`）、掉落物改平面貼圖、取消暈影、雨滴粒子減量。`rb.rs` 的 `FANCY_GRAPHICS` 仍是常數。
   - 粒子（全部／減少／最少）：照 `RenderGlobal.doSpawnParticle` 的規則接到 `Game::named_particle` 與各個直接呼叫。
   - 各向異性過濾（原版開啟時圖集每格加 8 像素邊框並改線性取樣）、3D 立體（anaglyph）、進階 OpenGL（遮蔽查詢）、FBO 開關。
   - 平滑光照的「最小／最大」已接上 `rb.rs`，但 `mesher.rs` 裡尚未改走 `rb` 的舊繪製路徑（樓梯、柵欄、液體等）自帶 AO，
     關閉平滑光照時這些方塊還不會跟著變。
   - 半透明分段目前不做深度排序（1.7.10 也不排序，保持一致）。

1. **生物 AI 不是原版的任務系統**（使用者 2026-11-13：「生物特性有問題」）。`entity/mob.rs` 的各種生物行為多半是手寫的近似
   （例如苦力怕原本自己判斷距離膨脹），不是 `EntityAITasks` + 各個 `EntityAIBase` 的移植。這違反準則 1。
   應該做的是：移植 `EntityAITasks`（優先序、mutex、每 3 tick 重新評估）與常用任務（`EntityAISwimming`、`AttackOnCollide`、
   `Wander`、`WatchClosest`、`LookIdle`、`NearestAttackableTarget`、`HurtByTarget`、`Panic`、`Tempt`、`Mate`、`FollowParent`……），
   再逐一把生物改掛任務。苦力怕已先照 `EntityAICreeperSwell` 與 `EntityCreeper.onUpdate` / `interact` 改寫（含打火石點燃）。
   高壓苦力怕（閃電）仍未做。
1a. **附魔效果**：時運與絲綢之觸已經由 `drops.rs` 生效（玩家挖掘）。其餘尚未作用（鋒利／不死剋星／節肢剋星、擊退、燃燒、掠奪、
   效率、耐久、保護系列、水下呼吸、親水性、荊棘、力量／衝擊／火焰／無限、海之眷顧、魚餌）。
   掉落的另外幾條路徑還沒改走 `drops.rs`：`harvestBlock` 的覆寫（剪刀剪樹葉／草／藤蔓、冰、雪片仍用舊的 `item::drops`）、
   水沖掉火把與植物失去支撐時的掉落、樹葉腐爛。`item::has_subtypes` 是近似（`max_damage == 0`），應改為原版 `getHasSubtypes` 的表。
1b. **客戶端預測放置／破壞方塊**：原版客戶端會立刻改自己的世界，伺服器再確認。本專案仍等伺服器的快照（實測 25–100 ms）。
   做法已想好：客戶端改自己的區塊副本並記下原值與指令序號，伺服器在 `Event::Tick` 帶回已處理的序號；序號到了還沒收到該區塊的快照就還原。
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
- 本次：修正雙擊 exe 打不開（舊的 jar 路徑已刪除，現在也會找 `%APPDATA%\Sapling` 等啟動器目錄與 exe 旁），啟動錯誤改以對話框顯示；聲音與語言檔因此也找得到了。
- 本次：使用者雙擊 exe 仍找不到 jar（其環境與本機測試不同，推測 `APPDATA` 不可靠）。改用 Known Folder API、加上 `minerust_jar.txt` 與檔案選擇視窗；以清空所有環境變數、工作目錄設為 `C:\Windows` 的方式啟動 release 與 debug 驗證。
- 本次：查出真正原因——Sapling 資料夾只存在於 Claude 套件的虛擬化 AppData 裡。jar 與 assets 複製到 `game\`，exe 會往上找到它；啟動時印出 `Jar:` 路徑。
- 本次（附魔效果，進行中）：`Math.random()` 改用獨立產生器（`util::math::math_random`），`ItemEntity::new` 不再消耗世界的 rand（原版就是如此，掉落物的亂數順序因此能與原版一致）。新增 `tools/DropHarness.java` → `tools/drops_1710.txt`：以只記錄生成物的假 World 執行原版 `dropBlockAsItemWithChance`（每個方塊 × meta × 時運 0–3 × 4 個種子，含經驗球與最後一個 `rand.nextInt` 以確認亂數順序），另有 `canSilkHarvest` 與 `createStackedBlock`。
  **下一步**：寫 `src/drops.rs`（`quantityDroppedWithBonus`／`getItemDropped`／`damageDropped`／各方塊的 `dropBlockAsItemWithChance` 覆寫、`harvestBlock` 的絲綢之觸與剪刀），以這份資料寫測試直到全部一致，取代 `item::drops`；再接時運、效率、親水性、耐久（`EnchantmentDurability.negateDamage`）。
- `本次（效能整體修繕，2026-11-13）`：使用者回報效能比原版差。加了基準模式（第 5.3 節）實測：RTX 5070 上不限幀只有約 100 fps，
  每幀 4 ms 花在區塊泵送、3.3 ms 在渲染。**根因**：伺服端只要有任何方塊或光照變動（隨機 tick、草蔓延、甘蔗長高……）就把整個
  16×256×16 區塊柱連同鄰居標髒並送出快照，客戶端收到後把 3×3 共九柱全部重新網格化——閒置世界 10 秒內 4,771 份快照、
  22,000 次整柱上傳，每次上傳各自 `vkAllocateMemory`，頂點還放在主機可見記憶體；載入時每柱平均重建 19 次。
  **修正**：(1) 髒標記改為 16³ 分段遮罩（原版 `markBlockForUpdate` 的 ±1 格範圍），(2) 網格器按分段輸出、只複製需要的高度範圍，
  (3) 渲染器改為分段 + 顯存池 + 暫存緩衝上傳、每分段獨立視錐剔除、同一緩衝內用 `vertexOffset` 繪製，
  (4) 九柱都 `ready` 才第一次網格化，(5) 方塊實體只重掃變動分段。結果見第 5.3 節的表（視距 16：97 → 1,311 fps，載入 24 → 7 秒）。
  **同時接上原本完全沒作用的影像設定**：垂直同步（`FIFO`／`IMMEDIATE`）、最大幀率（`FrameSync`，預設 120、沒有世界時 30）、
  平滑光照等級（改動後重建全部分段）、雲的開關與視距 < 4 不畫雲、視角晃動開關、Mipmap 層級、全螢幕（F11 與設定）。
  **驗證**：`cargo test --release` 82 項通過；改動前後同位置截圖逐像素比對（1280×720，差異 17 像素，為粒子時序）；
  載入後以 `/setblock` 放置跨分段、跨區塊邊界的方塊柱與水，確認增量重建正確；F3 顯示 121 fps（上限 120）。
  **留下**：第 7 節第 0 項的影像設定缺口；`Arc<ChunkData>` 在送出快照後的下一次寫入會整塊複製（262 KB），目前伺服器 tick 5 ms 尚可，
  之後可改成分段 `Arc`；區塊生成後的裝飾仍是單執行緒（原版的全序要求）。
- `本次（使用者回報的手感問題，2026-11-13 晚）`：使用者列出「實體抽搐、掉落物上下跳與穿模、生物移動卡頓、放方塊有音效但過很久才出現、
  貼圖用錯、有些東西反而更快」。逐項查證：
  - **掉落物／箭／TNT 等抽搐**：它們的 `prev`、`pos` 來自伺服器 tick，卻用客戶端 tick 的 `partial` 插值，兩個時鐘相位不同就會鋸齒狀前後跳；
    掉落物的浮動用 `age + partial` 也一樣。改用 `server_partial()`，並讓一個伺服器 tick 的事件整批套用（見第 4 節「兩個時鐘」）。
  - **掉落物穿模**：繪製時多減了 0.125（`posY` 已含 `yOffset`，`RenderItem.doRender` 是 `posY + 浮動`），方塊物品因此沉進地面。已移除。
  - **生物移動卡頓**：加上 tick 相位鎖定（同上）。
  - **放方塊延遲**：上一輪之前網格器被整柱重建塞爆（需求約 4,300 柱／秒、產能約 2,200），玩家所在區塊排不到。
    現在實測「右鍵 → 新網格完成」25–42 ms（`MINERUST_DEBUG_LATENCY=1` 搭配 `--open use:x,y,z` 會印出）。
  - **貼圖用錯**：新增 `tools/IconHarness.java` 匯出原版全部方塊貼圖並逐項比對，查到並修正：木板六種全畫成橡木（連帶五種木樓梯）、
    石英方塊的雕紋與柱狀變體、乾草捆橫放的朝向、平滑雙層半磚的砂岩款，以及一批中繼值越界時的回退貼圖（樹苗、砂岩、石磚、花、小麥、
    胡蘿蔔／馬鈴薯、蠹魚石）。尚未移植的方塊（測試會跳過）：26, 36, 63, 68, 92, 95, 104, 105, 107, 125, 126, 130, 137–139, 144,
    146–151, 154, 156–158, 160。
  - **「反而更快」**：沒有找到確定的單一原因。查過挖掘延遲（`blockHitDelay`）、右鍵延遲、排程 tick，都與原版一致。推測是先前伺服器
    執行緒被網格工作搶走 CPU，落後後連續補 tick（與原版 `MinecraftServer.run` 相同的追趕機制）造成世界忽快忽慢；上一輪修正後
    伺服器 tick 平均 5 ms，不再落後。**這一項沒有被證實**，需要使用者指出具體是哪個東西變快。
  **驗證**：`cargo test --release` 83 項通過；截圖確認六種木板、石英變體、乾草捆、木樓梯貼圖；基準 1,331 fps（視距 16，未退步）。
  抽搐與卡頓的修正只有編譯與邏輯推導，**沒有錄影逐幀驗證**。
- `本次（第二批回報 + 掉落表，2026-11-14）`：
  - **方塊掉落表**：`src/drops.rs` 完成，與原版 4 萬多筆參考資料全部一致（測試 `drops_match_the_game`），接到玩家挖掘（含時運、絲綢之觸）、
    爆炸、活塞。礦石經驗、蠹魚石放出蠹魚也走同一條路。
  - **點燃的 TNT**：閃爍照 `RenderTNTPrimed` 改成純白（原版 `renderBlockAsItem` 自己把顏色設成 alpha 1，所以那個 0.8 的透明度其實沒作用，
    每隔 5 tick 整塊變白），舊寫法是往白色內插 80% 且保留貼圖。「陷進下面的方塊」查不到位置錯誤（實體中心、0.98 的碰撞箱都對），
    判斷是上一輪修掉的插值抖動；截圖確認現在坐在方塊上。
  - **紅石線**：`mesher.rs` 的 `wire` 原本只有平面部分、連接規則是近似。已照 `renderBlockRedstoneWire` 與
    `BlockRedstoneWire.isPowerProviderOrWire` 重寫：中繼器只連前後、比較器／陷阱箱／陽光感測器等會連、往上連要求旁邊是實心方塊、
    孤立的線是中央小點而不是整個十字、爬上方塊側面的線段。
  - **放方塊「有音效但放不下來」**：客戶端沒有做原版 `ItemBlock.onItemUse` 的檢查就播音效並送出指令；方塊與玩家（或生物）重疊、
    或右鍵的是箱子等可互動方塊時，伺服器拒絕，按住右鍵要到下一次重複才成功。現在 `Game::placement` 先檢查
    （`canPlaceEntityOnSide`：可取代、不與玩家和生物的碰撞箱相交），不通過就不播音效、不送出，與原版一致；伺服器端改用方塊真正的碰撞箱。
    真正的「立即出現」還需要客戶端預測（第 7 節 1b）。
  - **苦力怕**：打火石右鍵點燃（`fire.ignite`、耗損 1、之後固定膨脹到爆炸），膨脹邏輯照 `EntityAICreeperSwell`
    （3 格內開始、超過 7 格或看不見才消退、膨脹時不移動）。測試 `flint_and_steel_ignites_a_creeper`。
  **驗證**：`cargo test --release` 85 項通過；截圖確認紅石線（十字、直線、爬牆、被火把點亮）與 TNT 的白色閃爍；基準 1,355 fps。
  放置檢查與苦力怕的實際手感沒有實機操作驗證。
  **查到的既有錯誤，待修**：伺服器端玩家的盔甲值從未設定（`living.armor` 恆為 0，`inventory.armor_value()` 沒人呼叫），盔甲不擋傷害也不耗損；玩家爆擊用了舊版隨機加成，1.7.10 是 ×1.5；武器附魔（鋒利系列、擊退、燃燒、掠奪）、保護系列、水下呼吸、荊棘、弓的附魔、附魔光澤都還沒接。
