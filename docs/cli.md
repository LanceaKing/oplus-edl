# CLI 用法

二进制名为 `oplus-edl`。用 `oplus-edl --help` 查看命令，用 `oplus-edl flash package --help` 查看某个子命令的参数。

## 命令目录

表内的方括号表示可选参数组，竖线表示二选一。多个 LUN、分区或 XML 文件分别用同名选项重复传入。

| 命令 | 用途 |
| --- | --- |
| `devices [--backend usb\|serial]` | 枚举指定后端的候选设备。默认 USB。只枚举候选。 |
| `partitions [连接参数] [--lun N ...]` | 建立一次会话并检查 GPT。省略 `--lun` 时检查设备报告的 LUN。 |
| `backup [连接参数] --output DIR (--partition LUN:NAME ... \| --all) [--lun N ...] [--overwrite]` | 备份显式选择的分区，或用 `--all` 备份所选 LUN 范围内的所有分区。 |
| `restore image [连接参数] --image FILE --partition LUN:NAME [写入参数]` | 把一个镜像恢复到一个明确的分区。 |
| `restore directory [连接参数] --from DIR (--partition LUN:NAME ... \| --all) [--lun N ...] [写入参数]` | 恢复目录中的显式映射，或用 `--all` 恢复范围内有效的现有映射。 |
| `flash xml [连接参数] --program FILE ... [--patch FILE ...] [刷写参数]` | 依次处理 program 文件和节点，再依次处理 patch。 |
| `flash package [连接参数] --package DIR --nv-id ID (--prepare-output DIR \| --prepared DIR) [--flash-config FILE] [刷写参数]` | 生成或重开固件包，再检查设备项目和范围并刷写。 |
| `package prepare --package DIR --nv-id ID --output DIR [--flash-config FILE]` | 在新目录离线生成固件包产物。 |
| `cloud fetch --config FILE --output DIR [--create-identity]` | 根据配置获取并校验云端加载器资产，输出到新目录。 |

### 连接与范围

- `--backend usb|serial` 默认 USB。使用 `--device KEY` 指定 `devices` 返回的完整键。USB 可自动选择唯一候选。候选不唯一时返回输入错误。串口要求指定 `--device`。
- `--memory ufs|emmc` 默认 UFS。扇区默认值为 UFS 4096、eMMC 512。可用 `--sector-size 512|4096` 显式指定。
- `--wait-timeout SECONDS` 默认只枚举一次。正数限制等待时长，显式传入 `0` 则等到发现候选或收到停止请求。
- 本地初始化需要 `--loader FILE`。需要签名摘要的加载器可配合 `--signed-digest FILE`，完成 DIGEST 会话时另加 `--digest-signature FILE`。
- `flash xml` 和 `flash package` 可用 `--cloud-config FILE --cloud-output DIR` 获取初始化材料。这两个选项要一起使用，与本地初始化材料互斥。
- 范围选项 `--lun` 仅用于 `partitions`、`backup` 和 `restore directory`。每个分区用 `LUN:NAME` 指定。第一个冒号后的内容是完整分区名。重复目标或超出显式 LUN 范围的目标会被拒绝。
- 目录恢复读取 `lun<N>/<partition>.bin`。平铺目录需要唯一目标 LUN，可从显式分区或 `--lun` 得出。`--all` 使用目录已有的有效映射。备份遇到已存在的正式文件时，可用 `--overwrite` 同意替换。已有 `.partial` 保留。

### 写入与刷写

`restore` 和 `flash` 都需要此次调用提供 `--yes`。省略时，CLI 返回输入错误，无设备访问或终端确认。

- `--verify best-effort|required|none` 默认 `best-effort`。已确认的写入和验证状态分别记录。如果读取被明确拒绝，`best-effort` 可继续，最终报告验证未完成。`required` 在写入前拒绝无法验证的计划。在 `none` 下，结果将验证标为 `not_requested`。
- `flash xml --raw-size declared|file` 默认 `declared`。在 `file` 模式下，RAW 长度按文件末扇区向上取整，并以 XML 范围为上限。`flash package` 固定用 `file`。
- `--set-bootable LUN` 和 `--reboot` 是可选的刷写最终动作。前序执行和验证策略均允许时才发送。

## 准备和重开固件包

NV ID 是文件名 `META/super_def.<ID>.json` 中保留前导零的数字串。在原包的 `META` 中查看可选文件，根据固件说明选定变体。工具使用显式提供的 ID。

```sh
oplus-edl package prepare --package ./firmware --nv-id 10010111 \
  --output ./prepared --json -q

oplus-edl flash package --package ./firmware --nv-id 10010111 \
  --prepared ./prepared --device '<key>' --loader ./loader.elf --yes
```

如准备时传入了 `--flash-config FILE`，重开时提供相同配置。`--prepared` 需要原包、NV 定义、`selection.json`、生成的 XML 和 `super.img`。它会重建选择和排除规则，检查 XML 操作及路径绑定、原包 patch 操作和 super 布局。证据缺失、目录混用、路径迁移或操作不一致时，重新用新目录准备。新清单记录 patch 操作，重开时一并核对。旧清单沿用其中的原包 patch 路径，重开时检查这些文件当前的操作和范围。

重开使用已有产物和当前源文件。每次运行重新检查设备的 OCDT 项目和写入范围，再取得此次同意。工具保留用于读取和比较的打开句柄，并在准备、同意、写入和回读的边界检查源文件元数据。检测到变化会停止后续命令，保留已确认结果。操作期间请保持来源稳定。

## 云端配置

```sh
oplus-edl cloud fetch --config ./cloud-config.json --output ./new-assets --json -q
```

`cloud fetch` 使用配置中的查询和凭据。匹配配置必须唯一。多项匹配时设置 `profile_id`，零项时返回错误。CLI 从启动时的工作目录解析相对的 SWM 身份路径和发布目录。默认打开已有身份。首次使用 SWM 时，可以显式传 `--create-identity`，在配置路径创建新身份。已有文件会阻止创建。非 SWM 配置拒绝此选项。

## 输出和退出状态

默认在 stdout 输出进度与终态。用 `--quiet` 或 `-q` 隐藏进度，终态、错误和警告继续输出。`--json` 将 stdout 改为 JSON Lines。用 `--json -q` 取得单条终态。`--help` 和 `--version` 在 JSON 模式下各返回一条终态，`data` 包含 `topic` 和 `text`。

- 每条记录有 `schema_version`（当前为 1）、`kind`、`seq`、`operation_id`、`session_generation`、`operation`。序号和标识是十进制字符串。尚未创建的标识为 `null`。
- `kind: "event"` 包含 `phase`、`level`、`message`、`item_id`、`progress` 和 `dropped_logs`。
- `kind: "result"` 是最后一条记录，包含 `outcome`、`target`、`coverage`、`items`、`errors`、`warnings`、`outputs`、`stop_reason`、`input_validity`、`session` 和 `data`。项目标识、字节偏移和计数使用十进制字符串。LUN 使用数字。
- 项目记录执行状态、写入确认、验证状态和范围。错误与警告包含 `code`、`message`、`category`、`item_id` 和可选路径。输出包含 `path`、`kind` 和 `state`（`complete` 或 `partial`）。路径使用 `display`，非 Unicode 路径另含 `unix_bytes_base64` 或 `windows_utf16`。
- 会话包含 `disposition`、`reason` 及按序排列的 `completed_initialization_steps`。人类输出也从终态显示会话原因和完成步骤，包含 quiet 模式。退出前释放 CLI 自己打开的会话。
- `devices` 返回 `data.candidates`，候选包含 `key` 和 `backend`。完成的 `partitions` 返回 `data.snapshot`，失败或停止时 `data` 为 `null`。
- `package prepare` 返回 `data.preparation`，含 `origin`、`manifest`、`super_image`、`programs`、`patches` 和 `exclusions`。 `origin` 含原包目录、profile、NV ID、项目号和定义文件。
- 成功的 `cloud fetch` 返回 `data.assets`，含 `loader`、`digest`、`signature`、`manifest` 和 `profile_id`。包准备或获取失败、停止时，`data` 为 `null`，保留的输出和新建的身份文件进入 `outputs`。备份、恢复、刷写和输入拒绝的 `data` 为 `null`。

| 退出码 | `outcome` 或交付状态 |
| --- | --- |
| 0 | `success` |
| 2 | `input_error`，含同意缺失 |
| 3 | `unsupported` |
| 4 | `execution_failed` |
| 5 | `verification_incomplete` |
| 6 | `unknown_write` |
| 7 | `input_changed` |
| 130 | `stopped` |
| 74 | stdout 交付失败 |

同时存在多个结果事实时，优先级从高到低为 `unknown_write`、`execution_failed`、`input_changed`、`input_error`、`unsupported`、`verification_incomplete`、`stopped`、`success`。终态保留实际完成的项目及其错误。

Ctrl+C 发出协作停止请求。在途应答遵守首次停止后的十秒宽限期。后续协议命令在事务起点检查停止。USB 在途读取计时待实测。stdout 断管时活动任务记录 `consumer_lost`，其他写错误或五秒无写入进度时记录 `output_failed`。CLI 保留核心事实并以 74 退出。帮助、版本和参数错误使用同一交付期限。自动化应同时检查终态和退出码。
