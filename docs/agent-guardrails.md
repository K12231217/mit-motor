# Agent 协作协议（本仓落地版）

> 来源：Cursor 工程师 Lauren 的分享（2026-11-19 笔记，X 原帖，视频 56:08）。原文按「硬门禁 / 软协议」
> 分四层强制；本文只保留能在本仓机器化的部分，每条写清**落点、命令、限制**。
> 上位：`AGENTS.md`（红线 PRE/MECH）、`docs/INDEX.md`（事实与 ADR）。本文不复制任何数值或引脚。
> 状态口径：**PASS** = 有命令且当前通过；**有限** = 有命令但覆盖范围见「限制」；**软** = 靠 Review 纪律，
> 不伪装成 Lint。

## 0. 强制层级与本仓落点

| 层 | 原层 | 本仓机制 | 强度 |
|---|---|---|---|
| 1 架构与类型 | — | 五 crate 目录、`Snapshot<T>` / 有界队列、编译期 `size_of`/`offset_of!` 断言、enum 静态分发 | 硬（编译失败） |
| 2 CI / 编译器 | H1–H5 | `cargo xtask arch` / `clippy` / `hygiene` / `tier` + `.github/workflows/ci.yml` | 硬（退出码 1） |
| 3 Agent 约束 | S1–S3 | `AGENTS.md` 红线、本文、Thread 路由 | 软 |
| 4 人工 Review | — | 本仓无第二人；用「两次提交 + 台架签收」替代 | 软 |

原则：能下沉到第 1–2 层的抱怨一律下沉；下沉失败的原因写进「限制」，不留在口头约定里。

## 1. 硬门禁

| ID | 规则 | 命令 | 状态 |
|---|---|---|---|
| H1 | 依赖只能按 `contracts ← user ← app` 单向 | `cargo xtask arch`（I1） | 有限：只覆盖已存在的 crate |
| H2 | 错误与未完成项不得静默 | `cargo xtask clippy`（两个 workspace，`-D warnings`）+ `cargo xtask hygiene`（H2） | PASS |
| H3 | 硬件耦合改动无台架证据不得进 main | `cargo xtask tier [base]` | PASS（当前分支按设计 FAIL，见 H3） |
| H4 | 新模块必须写进文件架构唯一真源 | `cargo xtask hygiene`（H4） | PASS |
| H5 | 注释不得承载历史、流程状态或补丁合理化 | `cargo xtask hygiene`（H5）+ `docs/comment-style.md` | 有限：按行级注释文本判 |

### H1 依赖边界

- 落点：`cargo xtask arch` 的 I1（`cargo tree -p user` 里不得出现 `bsp`/`embassy-*`/`rtic`）、
  I2（`0_contracts` 与 `3_user` 零 `unsafe`）。
- 限制：`1_bsp` 已建（T-011 脚手架），I2 仍只扫零 `unsafe` 的 `0_contracts`/`3_user`
  （`1_bsp` 是 `unsafe` 白名单，不进 I2）；`hygiene` 的扫描根已含 `1_bsp/src`。
  `2_system`/`4_app` 落地时必须把 `checks.rs` 的 `SCAN_ROOTS` 与
  `docs/software-architecture.md` §9-8 一并扩展，否则不得报 PASS。

### H2 静默吞错与无主 TODO

- 编译器层：两个 crate 根 `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]`；
  `cargo xtask clippy` 现在跑 firmware 与根 workspace 两遍，测试目标也在内。
- 标记层：`hygiene` H2 扫 `0_contracts/src`、`1_bsp/src`、`3_user/src`、`firmware/src`，规则：
  - `TODO` / `FIXME` 只能是 `TODO(owner, trigger): 说明`，缺 owner、缺触发条件或格式不符即 FAIL；
  - `XXX` / `HACK` 无条件 FAIL——它们本身就是「先这样，以后再说」。
- 限制：行级扫描，字符串字面量里出现 `//` 会误判（四个源码树当前无此写法）；`xtask` 自身不扫。

### H3 分级门禁

判定按路径，不按人自觉：

| Tier | 路径 | 合入条件 |
|---|---|---|
| A | 其余（`0_contracts/`、`3_user/`、`docs/`、`xtask/`、`.github/`） | Host 测试 + 固件编译 + `arch/hygiene` 全绿；`HIL: NOT_RUN` 可接受 |
| B | `firmware/`、`1_bsp/`、`2_system/`、`4_app/` | 必须有台架证据：`.scratch/handoff.md` §3 里独占一行、以 `HIL: PASS` 开头的记录 |

- 命令语义：`cargo xtask tier [base]` 比较 `base...HEAD` 的改动文件名；`base` 缺省取
  `$TIER_BASE`，再退 `origin/main`。基线是空提交（新分支）直接 PASS。
- Tier B 边界：`firmware/memory.x`（链接器内存区）与 `firmware/Cargo.toml`（目标/依赖）
  同属硬件耦合，不是「只改配置」。本分支 `feat/contracts-and-gate` 就是活例：两个文件在改动里，
  没有 HIL 记录，`tier` 必须 FAIL。
- 整改只有两条：留在 `hil-pending` 分支 / Draft PR；或补实测记录。没有第三条「本地跑一下就算」。
- 限制：本仓无分支保护，直接 push main 时基线是 `github.event.before`，能查出「这次推了什么」，
  但查不出「谁在本地跳过了 PR」。硬拦截的信任边界止于此，写在这里而不是假装没有。

### H4 文件架构不腐烂

- 原文用「自动生成 FEATURE_MAP.md + `--check`」。本仓**不生成第二份地图**：`docs/software-architecture.md`
  §2 的文件树是唯一真源（PRE-10），生成物会立刻变成第二个漂移点。
- 落点：`hygiene` H4 把每个 `<crate>/src/**/*.rs` 与其 crate 的文件树块比对，缺行即 FAIL。
  新增模块那一刻就必须改文档，而不是等 review 发现。
- 限制：按字符串包含判定；同名文件（各 crate 的 `src/lib.rs`）靠 crate 分块隔离，跨块串联只放宽不放严。

### H5 注释不放行补丁

- 原文默认禁止「解释实现逻辑的 Why 注释」，只留 `ERRATA:` / `REF:`。本仓**不照搬**：
  `docs/comment-style.md` §12 要求写 Why（物理依据、不变量、安全边界），传感器/电机代码里多数
  Why 没有外部编号可指。取交集，机器只拦可判定的部分：
  - 块注释 `/* */`（只允许 `//` / `///` / `//!`）；
  - `// SAFETY:`（本仓靠 `forbid(unsafe_code)` 拦截，不写安全注释）；
  - ASCII 分隔线；
  - 历史/流程词：`旧实现`、`曾用`、`已作废`、`已废弃`、`评审`。
- 禁止清单之外的「补丁合理化」仍靠 S1 与 review：注释解释的是约束，不是辩解。

## 2. 软协议

### S1 根因结论 = 证据 + 证伪路径

任何「就是这个原因」必须给出两样东西，缺一不算结论：

1. 证据：`文件:行号`、命令原文输出、日志、波形，或最小复现脚本；
2. 证伪路径：如果判断成立，修完后在哪个可观测指标上会怎样变化；不成立时哪个现象会保留。

禁用形态：贴一段代码 + 一句自信推断。对应 `AGENTS.md` 认知纪律 §1（假说须证伪）与「闭环铁律」
（无外部证据只能报「已修改，未验证」）。

### S2 一个 Thread 一件事

- 一个 Thread 一个可验收单元，`git revert` 必须能无痛回退；
- 脚手架与实现分两次提交（`wip(<thread>): 冻结脚手架` → 实现），混在一起即视为腐化；
- 不设行数上限：50 行和 1000 行都可以，标准是「职责单一」。

### S3 升级为硬规则的 ROI 门槛

| 类别 | 触发条件 | 动作 |
|---|---|---|
| 高危 | 首次出现：静默错误、越界、烧管/失控 | 立即下沉到第 1–2 层（类型、编译期断言或 xtask 检查） |
| 高频 | 两周内同类低级错误 ≥ 2 次 | 下沉；先写进「不二过台账」，再补检查 |
| 冷门 | 半年一次 | 留在 S 层，不写 AST/Lint |

现成的判决记录在 `AGENTS.md` §4 不二过台账，新增条目必须回答「为什么不加机器检查」。

## 3. Skill 回归（个人项目版）

不搞多路 subagent 盲测与 LLM 打分。每个核心规则绑定 2–3 个**来自真实提交**的最小复现，
判分只看退出码：

| # | 来源 | 复现动作 | 判定 |
|---|---|---|---|
| 1 | T-002 依赖落地 | 往 `3_user/Cargo.toml` 加 `1_bsp` 路径依赖 | `cargo xtask arch` 的 I1 必须 FAIL |
| 2 | T-005 新增模块 | 新建 `3_user/src/foo.rs` 并声明 `mod foo;` | `cargo xtask hygiene` 的 H4 必须 FAIL |
| 3 | 本协议 H3 | 改 `firmware/memory.x` 后跑 `cargo xtask tier` | 无 HIL 记录时必须 FAIL |

新增检查时同步加一行；检查失效即复现失败，说明它已腐烂——删掉重写，不要留着装门面。

## 4. 明确不照搬

- **不照搬无限 Token 的暴力重构**：不派上百个 Agent 刷 PR；投入只给长期复用的公共底座（xtask、契约、类型）。
- **不照搬前端具体禁令**：只迁移方法（把高频/高危抱怨下沉为架构约束与确定性检查），不搬 React/Electron 规则。
- **不采信营销口径**：「20+ 全自主 Agent」「99% / 1%」等非原话内容不进入本仓实践。
- **不生成 FEATURE_MAP.md**：见 H4，第二份地图就是第二个漂移源。

## 5. 维护与失效条件

- 本文只写机制与命令，不复写阈值、引脚、容量（PRE-07/PRE-10）。
- H1 的扫描根随 `2_system`/`4_app` 落地而扩展；`1_bsp` 只进 `hygiene`，不进 I2；未扩展前 `arch` 不得宣称覆盖四层。
- H3 的 Tier 路径表在出现第一个「硬件耦合但不在四个前缀下」的目录（例如新的 `5_*`）时立即失效，
  必须同步 `xtask/src/tier.rs` 的 `TIER_B_PREFIXES`。
- H4 依赖 `docs/software-architecture.md` §2 的 crate 分块格式；文件树改成表格或列表时，
  `hygiene` 会整体报「缺块」——那是格式变更信号，不是脚本坏了。
