# MINERUST 接手手冊（給下一位開發者／AI）

接手前請從頭讀完。這份文件只寫**不常變的東西**：目標、硬性準則、參考資料、架構、驗證方法、注意事項。
會隨每次推送變動的內容各有一個固定的家，不要寫進這裡：

| 文件 | 內容 | 什麼時候更新 |
| --- | --- | --- |
| [PARITY_TODO.md](PARITY_TODO.md) | 唯一的「還沒做完」清單與優先順序 | 每次推送（做完的刪掉、新發現的加上） |
| [CHANGELOG.md](CHANGELOG.md) | 每次推送一行：提交編號與標題 | 每次推送 |
| [BUG_AUDIT.md](BUG_AUDIT.md) | 原版 bug 票的處理狀態 | 修了原版 bug、或實作了票所在的系統時 |
| [../README.md](../README.md) | 使用方式、已移植系統的概況、比對結果 | 大項完成或使用方式改變時 |
| 本文件 | 目標、準則、架構、流程、注意事項 | 這些本身改變時 |

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
   每次提交後推送；**每次推送同時更新 CHANGELOG.md 與 PARITY_TODO.md**，其他文件照上表在需要時更新。
   同一件事只寫在一個地方，其他地方用連結指過去；寫之前先看該文件裡有沒有已經過期、彼此矛盾的句子。
8. **一口氣做完，不要停下來問使用者**；回報用繁體中文。
9. 下載或解壓的外部檔案視為不受信任資料；不要在其目錄內執行任何程式。
10. **效能是需求的一部分**（使用者 2026-11-13：「性能甚至比原版還要垃圾」）。動到渲染、網格化、區塊傳送、
    伺服器 tick 的改動，提交前跑第 5.3 節的基準測試，與上一個提交**同一時間**輪流量；變差就先查原因，
    確定是原版本來就要做的工作才接受，並把數字寫進提交訊息。

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
| `src/render/` | Vulkan 1.3（`vk.rs`：dynamic rendering，沒有 render pass 與 framebuffer 物件；屏障一律用 synchronization2 的 `cmd_pipeline_barrier2`，提交用 `queue_submit2`。新技術只改「怎麼畫」，換掉之後必須用截圖逐像素比對，畫面不能有任何變化）、方塊網格化（`mesher.rs`、`rb.rs` 是 `RenderBlocks` 的機械式移植）、模型（`model*.rs`）、箱子、告示牌、圖集 |
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
- **客戶端預測**（`Game::predict` / `send_dig` / `use_on` / `settle_predictions`）：挖掉方塊與放下普通方塊時，客戶端立刻改自己的區塊副本並重建
  網格（原版 `onPlayerDestroyBlock`、`ItemBlock.placeBlockAt` 在客戶端的行為）。每個 `Command::Dig` / `Command::Use` 兩端各自編號
  （`Game.sent_actions`、`sim::ACTIONS_DONE`），`Event::Tick(時間, 已處理數)` 把伺服器的進度帶回來：
  收到該區塊的快照時，若動作已被處理就以快照為準並丟掉預測，否則把預測重新蓋到新快照上（避免舊快照讓方塊閃回去）；
  一個伺服器 tick 套用完後，已處理但沒有收到快照的預測代表被伺服器否決，還原成原值。
  `MINERUST_DEBUG_LATENCY=1` 會印出每個預測的結果，`MINERUST_PREDICT_BOGUS=1` 故意多預測兩個伺服器不會放的方塊來測還原。
- **生物 AI**（`src/entity/mob/ai.rs`，是 `mob.rs` 的子模組）：
  - `Tasks` = `EntityAITasks`（每 3 tick 全面重評、`canUse` 的優先序與 mutex、`executingTaskEntries` 依啟動順序）；每隻生物有
    `tasks` 與 `target_tasks` 兩份。`Ai` 列舉的每個變體就是一個 `EntityAIBase` 類別連同它的欄位，
    `should_execute` / `continue_executing` / `start_executing` / `reset_task` / `update_task` 與原版五個方法一一對應，亂數呼叫順序照原版。
  - `ai::update` = `EntityLiving.updateAITasks`：targetTasks → tasks → `nav_update`（`PathNavigate.onUpdateNavigation`，含直線捷徑
    `isDirectPathBetweenPoints`、卡住 100 tick 放棄、`removeSunnyPath`）→ `move_helper` → `look_update`（`EntityLookHelper`）。
  - `ai::old_update` = `EntityCreature.updateEntityActionState` + `EntityLiving.updateEntityActionState`（舊式 AI）。
  - 目標以 `ai::Target`（玩家或生物編號）表示：`attack_target`（`getAttackTarget`）、`revenge`（`getAITarget`，100 tick 後遺忘）。
    生物之間不能在同一 tick 互相改狀態，所以攻擊別的生物走 `Strike`，`HurtByTarget` 的呼叫同伴走 `Outcome.calls`，迴圈結束後套用。
  - `build()` 是各生物建構子裡的 `addTask` 清單；新增生物時照原版建構子抄優先序。`uses_tasks` / `uses_old` 決定走哪一條路，
    其餘生物仍走 `tick_mob` 裡舊的 `match`。
- **生物的實體基底**：`Mob::move_entity` 借用 `base::Ent::move_entity`（物品、礦車、船共用的 `Entity.moveEntity` 逐行移植），
  再套生物的覆寫（步高、`canTriggerWalking`、`playStepSound`、`setInWeb`、各職業的 `fall`）；`Ent` 的 `living` 打開
  `EntityLivingBase.updateFallState`（落地塵土 `Fx::Dust`、`Fx::Landed`），`own_steps` 讓生物播自己的腳步聲。
  `Mob::entity_update` 是 `Entity.onEntityUpdate` + `EntityLivingBase.onEntityUpdate`（水、燃燒、熔岩、虛空、窒息、空氣）。
  移動中不能改世界，所以踩壞農田、點亮紅石礦石、鍋釜滅火先記成 `WorldOp`，由 `drain_moves` 在 `tick_mob` 裡執行。
  生物之間與生物和玩家的推擠是 `Mobs::push_apart`（`collideWithNearbyEntities`）。
  **不要再為生物或玩家另寫一份簡化的移動或碰撞**：先找 `base.rs` 有沒有現成的逐行移植。
- **伺服器端的玩家**位置來自客戶端（`Command::Player`），不跑 `moveEntity`；`ServerPlayer::moved` 之後由
  `block_contacts` 補上 `moveEntity` 尾段（火、仙人掌、鍋釜）、`base::landing` 補上落地（踩農田、塵土）。
- **近戰與附魔**：客戶端只送 `Command::Attack { id, crit, sprint }`（`crit` 是客戶端的墜落狀態），
  伺服器的 `Sim::attack_mob` 是 `attackTargetEntityWithCurrentItem` 的移植：攻擊力屬性加上手持武器的修飾值（`item::weapon_damage`）、
  `enchant::modifier_living`、擊退、燃燒、命中後的爆擊粒子事件（`Event::CritFx` → 客戶端的 `EntityCrit2FX` 發射器，`magicCrit` 是附魔爆擊）、
  `setSprinting(false)`（`Event::StopSprinting`）、武器耗損、掠奪等級記在生物身上（`Mob.looting`，`onDeath` 用）。
  原版客戶端對生物的 `attackEntityFrom` 一律回傳 false，所以客戶端**不**減速、不自己出爆擊粒子；船與礦車例外（客戶端回傳 true）。
  玩家身上的盔甲在一個 tick 內由 `ServerPlayer.armor_items` 持有（`Sim::armor_in` / `armor_out` 與物品欄同步），
  受傷時當場耗損（耐久附魔用玩家自己的 `e.rand`），保護附魔的隨機部分用 `EnchantmentHelper` 自己的靜態亂數（`enchant::enchantment_rand`）。
  `Item.itemRand` 是 `util::math::item_rand`。沒有攻擊者的傷害一律走 `Mob::hurt_src(量, 可被護甲擋, 火焰, 溺水)`：
  它照 `attackEntityFrom` 在新的一擊時播受傷音效，火焰傷害在抗火藥水下無效，溺水不呼叫 `setBeenAttacked`。
- **隨機 tick 與成長**（`src/world/growth.rs`）：`ticks::ticks_randomly` 決定哪些方塊在隨機 tick 呼叫 `updateTick`；
  作物、莖、樹苗、地獄疣、可可、耕地等在 `ServerWorld::growth_tick`。隨機 tick 期間世界亂數被「借出」（`std::mem::replace`），
  所以這裡的函式一律用傳進來的 `r`，掉落用 `drop_block_with(r, ..)`，不能呼叫會用 `self.rand` 的 `drop_block_with_chance`。
  樹苗長成樹時 `ServerWorld.gen_notify = true`，讓樹木產生器的 `set` 用 flag 3（`WorldGenerator(true)`）。
  骨粉是 `ServerWorld::bone_meal`，客戶端的綠色粒子走 `Event::BoneMeal`。
- **床**（`src/sim_bed.rs`）：右鍵床由 `Sim::use_bed` 處理（需要玩家位置與附近的怪物，所以不在 `play.rs`）；`Sim.sleeping`、
  `sleep_timer`、`bed_spawn`（存在玩家 NBT 的 `SpawnX/Y/Z`）。睡滿 100 tick 就跳到隔天早上並清掉雨（`resetRainAndThunder`）。
  客戶端：`Event::Sleep` → `Game.sleeping`、凍結玩家物理、開 `GuiSleepMP`（`chat_screen` + `sleep_gui`）、畫面變暗、
  `orientCamera` 的躺姿（平移 0.3、轉向床的方向、`yOffset - 1.62 + 1`）。左鍵開始挖掘時送 `Command::ClickBlock`
  （`onBlockClicked`：吃蛋糕、彈音符盒、打熄火）。
- 客戶端只有伺服器送來的快照；容器點擊在客戶端用同一份 `container.rs` 預測，伺服器的 `clicks` 計數讓客戶端丟掉過時的快照。
- 生物在客戶端有自己的副本：伺服器端 `Mob::track()` 模擬 `EntityTrackerEntry`（1/32 座標、4/32 門檻、S19 頭部），
  客戶端以三步內插（`setPositionAndRotation2`）與肢體擺動自行計算；身體朝向在 `game::update_body`：
  任務 AI 的職業（`isAIEnabled`）用 `EntityBodyHelper` 讓身體跟著頭，其餘用 `EntityLivingBase.updateDistance`。
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

最後的數字是載入完成後等幾**秒**才截圖。截圖與基準都從**目前工作目錄**讀 `options.txt`，要比較不同設定時從不同的暫存資料夾執行。

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
| 給物品、效果、召喚 | `--give id:meta:count,...`、`--effect id:秒:等級`、`--summon Cow,Zombie:baby,Zombie:villager,Item:id:數量`（距離 `MINERUST_SUMMON_DIST`） |
| 放方塊 | `--setblock "x y z id [meta];..."`（絕對座標） |
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
| 附魔效果（保護 × 傷害來源、鋒利系列 × 生物屬性、耐久、荊棘）與生物死亡掉落（33 種 × 掠奪 0–3 × 擊殺者是否玩家 × 著火） | `tools/EnchantFxHarness.java` → `tools/enchant_fx_1710.txt`（測試 `effects_match_the_game`、`death_loot_matches_the_game`；生物以假 World 建構後直接呼叫 `dropFewItems` / `dropRareDrop`） |

新增系統時照同樣模式：寫 Harness → 存文字檔 → Rust 測試比對。

### 5.3 效能基準（動到渲染或區塊流程必跑）

```
MINERUST_BENCH=10 minerust --seed 12345 --no-save --size 1920 1080 --distance 16 --time 6000 --freeze-time
```

世界載完後關閉垂直同步與幀率上限、鏡頭緩慢旋轉，量 10 秒，印出：載入秒數、平均／中位數／99%／99.9%／最大幀時間、
每幀在「區塊泵送／tick／組幀／渲染」各花多少、伺服器 tick 平均與最差、區塊快照與分段上傳的總數。
`MINERUST_BENCH_VSYNC=1` 保留垂直同步，`MINERUST_BENCH_STILL=1` 鏡頭不轉。
`MINERUST_BENCH_TNT=6` 在量測開始時於玩家東邊八格放一個 6×6×6 的 TNT 並點燃其中一顆（216 顆連爆），看最大幀時間；
`cargo test --release tnt_cube_timing -- --ignored --nocapture` 量同一件事的伺服器端 tick 時間。

基準（RTX 5070 Laptop、1920×1080、種子 12345）：

| | 視距 12 | 視距 16 |
| --- | --- | --- |
| 改動前（`e87f170`） | 106 fps，載入 4.6 秒 | 97 fps，載入 24.0 秒 |
| 分段網格 + 顯存池之後 | 1,575 fps（平均 0.64 ms，99% 2.4 ms），載入 3.9 秒 | 1,311 fps（平均 0.76 ms，99% 2.7 ms），載入 7.1 秒 |

伺服器 tick：視距 16 閒置平均 5.0 ms、最差 11.4 ms（預算 50 ms）。

**注意**：2026-11-14 起這台機器同一個 `bed27ea` 之後的版本只量得到 900–1,000 fps（上一個提交 `c6294d6` 重編後實測 960 fps、
伺服器 5.6 ms），與表中 1,311 fps 的差距是機器狀態（GPU 功耗只有 8 W，疑似電池模式），不是程式退步。
比較時請**同一時間**重編上一個提交一起量（`git worktree add ../base HEAD` + `CARGO_TARGET_DIR=target/base`），
兩個版本輪流各跑兩次。

最近一次（2026-11-20，`c57b73a`）：視距 16 約 900–1,000 fps；伺服器 tick 平均約 8 ms。這個種子在量測時有約 640 隻生物，
生物改走原版移動、推擠之後每 tick 多了約 1.3 ms（原版本來就要做的水流、窒息、空氣、推擠）。

## 6. 進度與待辦

- 已完成的系統：[../README.md](../README.md) 的「已移植」。
- 還沒做完的與優先順序：[PARITY_TODO.md](PARITY_TODO.md)。
- 每次推送做了什麼：[CHANGELOG.md](CHANGELOG.md)。

## 7. 已知注意事項

- 本機沒有 Vulkan validation layer；`--validate` 無法在此機驗證。
- 舊格式存檔（`level.txt`）不會出現在原版的世界選單，這是原版行為。
- 聊天打字與創造搜尋用 `text_input()` 走 `Game::text_key`，按鍵事件需要字元時記得接上。
- Bash heredoc 內含撇號會壞；修補腳本請寫成檔案再執行。
- **世界亂數的「借出」是結構性問題（未解決）。** 全專案有 31 處 `std::mem::replace(&mut w.rand, JavaRandom::new(0))`：
  把 `world.rand` 拿出來當參數 `r` 傳給 tick／生成函式，結束再放回去。借出期間 `self.rand` 是種子 0 的假產生器，
  所以**在借出期間被間接呼叫、又自己去用 `self.rand` 的程式**（最常見的是 `set_block` → `on_neighbor_change` →
  `check_and_drop` / `drop_block_with_chance`）抽到的是假亂數，而且抽完就被丟掉——原版這時用的是同一個 `world.rand`，
  亂數順序會因此與原版分岔（例：火燒掉花下面的方塊、莖旁邊結出南瓜時鄰居的檢查）。寫新程式時：手上有 `r` 就一路往下傳
  （`drop_block_with(r, ..)`、`destroy_block_with(r, ..)`、`fizz(.., r)`），不要在可能已借出的路徑上呼叫會自己借的函式。
  徹底的解法是把 `on_neighbor_change` / `on_block_added` 也改成接收 `r`，或讓 tick 函式直接用 `self.rand`、只在呼叫
  世界生成器（樹、巨型蘑菇）時借出；這是一次大範圍的機械式改寫，還沒做。
- **使用者說某處「和原版不一樣」時**，先查原版原始碼確認原版確實不是這樣（例：1.7.10 拉桿本來就放不上 TNT）；
  再找有沒有繞過共用移植的手寫平行版本（2026-11-19/20 的燃燒傷害、腳步聲、頭轉 180 度都是這個原因）。
  稽核舊程式時，grep 原版的 `dropBlockAsItem`、`playSoundEffect`、`super.` 呼叫，確認每一個都有移植。
- 不要從實體自己的 `rand` 抽編號或其他簿記用的亂數，會打亂原版的亂數序列（用 `Mob::projectile_id`、`Mobs.rand`）。
- 傷害新建 `ServerPlayer` 的單元測試要先把 `respawn_invulnerable` 歸零（加入與重生後 60 tick 無敵）。
- `target/release/minerust.exe` 可能正在被使用者執行而被鎖住：建置用另一個 `CARGO_TARGET_DIR`（例如 `target/alt`），
  驗證完再複製過去。注意 `cargo test` 不會重建執行檔，複製前要 `cargo build --release`。
