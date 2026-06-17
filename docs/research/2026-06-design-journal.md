# 设计日志：2026 年 6 月赎回机制的探索、死胡同与裁决

- **日期**: 2026-06-16 ~ 06-17
- **状态**: design journal（实录/非规范性；记录"我们想了什么、否了什么、为什么"）
- **作者**: Jeffery（方向 + 关键判断）+ AI（探索、对抗性辩论、研究）
- **配套**: 竞品/技术研究见 [landscape-and-bitvm-research](2026-06-landscape-and-bitvm-research.md)；规范结论见 `docs/spec/07-redemption-notes.md`。
- **说明**: 这是一份**curated 实录**——捕捉这几天密集讨论的实质内容与结论链，而非逐字对话。逐字原始记录见会话 transcript（见文末"原始记录"）。

---

## 0. 这几天的主线（一句话）

从"把 redeem_tx 这块基石做实"出发，一路撞到一个**结构性不可能定理**，用一场对抗性辩论判了两个自创设计的死刑，收敛到"v0 诚实分层"，再在绝望边缘做了一轮竞品/前沿技术调研——结论是：**不是死胡同，是一个被 Vitalik 刚背书、且在比特币上真空的格子，外加至少一条今天能搭的缓解路。**

---

## 1. redeem_tx 落地（已规范化进 spec 07）

- **结论**：redeem_tx = 两输入单笔 tx，`[Q(LP 的独立、超额抵押、纯 BTC、oracle-adaptor 预签的 DLC 输出) + A(持有人的 SatUSD TA UTXO)] → [A 的 SatUSD→burn sink，X/P→holder，找零→LP]`，**持有人独自广播、赎回时无 LP**。devnet 端到端验证通过（`satusd-rail1/tests/devnet_settle::redeem_two_input`）。
- **过程纠错**：一度把两输入误判为"非单边"（错把 Q-leg 当成 j4_settle 的现签 LP 输入），又试 combined 单输出（tapd 无法锚定大额 TA → 死胡同），最终回到两输入。
- **遗留的 v0 托管漏洞**：Q 是单密钥（`funding.rs:6` "v0 single key; MuSig2 upgrade"）→ 恶意 LP 可在赎回前挪走 Q。修复 = MuSig2 把 Q 改 2-of-2（已披露进 spec 07 §3.3/§7）。

## 2. DLC channels / factories 研究（已 PARK，记入 spec 07 §10.4/§10.6）

- **DLC channels**：链下 `renew` 能解决滚动预签的两个痛点，但代价是结构性的——(a) renew 双方每块交互（退回 LP 单边推送之倒退）；(b) 引入 watchtower；(c) 撤销+惩罚模型使**兜底比 v0 单笔赎回更弱**（旧状态 force-close 会被罚没整个 buffer，只对最新状态安全）。**结论：PARK，非 v0 路径；v0 单笔赎回在单边轴上严格更强。** 10101 实证：保留 DLC channel、放弃 LN 内嵌 → standalone。
- **DLC factories**：同一惩罚模型 → 离线成员资金有风险、可死锁 → 破单边赎回；且 Conduition 概念仅 2 方、无 N 方拆分树、无代码（TRL~1）。**PARK。**

## 3. 核心死胡同：free-option（旧授权不过期）

- **问题**：比特币没有"签名过期"机制；让旧交易失效的唯一办法 = 花掉它引用的输入。滚动预签时旧 CET 不失效 → 持有人可挑历史最低价赎回（free-option）→ 掏空超额抵押 → 打穿 peg。
- 探索过的失效机制全部失败或转化为信任：
  - **permissionless 滚动**：作弊者拒绝前进（冻结最优旧锚）；且 redeem 与 roll 双花竞态可被第三方抢跑改写赎回价。
  - **加一个中立方 P（与 oracle 一起滚锚）**：作废权 = 干预权——P 控制每笔赎回引用的锚 → 能拖延/审查/重定价，破单边。
  - **给 P 加质押/罚没（crypto-economic）**：危险偏离（抢跑/不作为/定向）不可证明 → 罚不到；可证明偏离的罚没在比特币上又需 CSFS 或可信法庭；押金 < 贿赂；唯一无信任可罚的是 **equivocation=自动泄密**（SatUSD 预言机已用此），但抢跑/审查不是自我指证型。

## 4. 不可能性定理（本月最硬的结论）

> **在比特币当前主网上，"期内 + 任意时刻 + 当前价 + 单边 + free-option=0"五者无法同时成立。任何今天的设计最多得其四，必弃其一。**

**因果链**：要"独自+任意时刻"赎回 → 授权必须事先在持有人手里（预签）→ 但脚本读不到实时价 → 预签授权只能写死某个价 → 覆盖"任意时刻当前价"就得预签一长串各价位授权 → 比特币没有签名过期机制 → 持有人攒下许多仍有效的授权 → 挑最有利的旧价（free-option）。强制只认最新价需作废旧授权，而作废要么需持有人配合（作弊者拒绝）、要么让第三方抢跑（破单边）、要么靠脚本验实时签报（CSFS，主网没有）。

**今天的取舍**：
- 弃"当前价"（只到期结算）→ 得「到期赎回+单边+free-option=0」= **干净的定期票据（Mode 1）**。
- 弃"free-option=0"（接受超额抵押覆盖）→ 得「期内+任意时刻+当前价+单边」= **超额抵押的活期（Mode 2，像 DAI 那样押抵押）**。
- 五个全要 → 需主网未激活的 CSFS / 见证加密 / 契约（路线 C）。

## 5. 对抗性辩论：B′ / B″ 被判死刑（11-agent 证伪工作流）

针对自创的"新鲜锚（fresh anchor）"设计 B′（per-note F + permissionless oracle-paced 滚动 + runway + maturity 地板 + 2-of-2 Q）做了一场 6 攻击 + 综合 + 3 红队 + 收敛的辩论。

- **三条致命伤（无一能在无 CSFS/covenant 时修）**：
  - **F1 未来-outpoint 死结**：runway 要预签引用未来 F 的 CET，但 SIGHASH_DEFAULT 焊死输入 outpoint、未来 F 的 txid 发行时不可知 → 签不出来。需 ANYPREVOUT/covenant。
  - **F2 permissionless 滚动对作弊者无强制力**：作弊持有人冻结最优锚不滚不赎 → free-option 上界从"滚动周期"发散到"整个持有期 σ(~40%)"→ 资本成本=路线 A，却多背全部复杂度。
  - **F3 第三方抢跑改写赎回价**：redeem 与 roll 双花竞态由 feerate 裁决、非协议裁决。
- **B″（精简版）也被否**：它=v0 改名，且偷偷复活了它声称要消灭的 σ-期权（删 F 后没有失效机制，持有人照样囤 LP 历史推送的预签 CET 取 max），还多加了多余的 Q_mat/Q_coop 拆分。
- **收敛裁决：v0-honest = 路线 A 的诚实分层。** 单个 Q（2-of-2 MuSig2，refund 改 holder-only）；到期赎回与现 devnet 路径同构；早赎做成**显式二选一产品模式**：Mode 1（纯到期票据，free-option=0）/ Mode 2（v0 推送票据，单边但 σ-期权，超额抵押覆盖）。
- **被点破的工程现实**：(1) **离线 maturity 地板从未 E2E**（spec 07 §10 仍 open）——这是不变量 4 唯一的真保证、最高优先级；(2) **MuSig2-adaptor 是未写的敏感新代码**（在它落地前"LP 偷不走 Q"是纸面）；(3) **CET 预签 nonce 派生**别照搬测试样板（同桶不同 event 复用 nonce=泄私钥）。

## 6. 情绪低点与平台之问（Vitalik / Ethereum / BitVM）

- 辩论余波里产生"项目没价值、不如去 Ethereum"的念头。澄清：那场辩论是**故意调到最毒的、只衡量健壮性不衡量价值**，在它的谷底给项目判死是范畴错误。
- 随后一轮竞品/前沿调研（详见 [研究报告](2026-06-landscape-and-bitvm-research.md)）得出三个反方向事实：
  1. **Vitalik 2026-06 的期权型无清算提案 = SatUSD 的经济模型**（P=持有人、N=LP、P+N≡1、慢速预言机、无清算）——SatUSD 是其比特币原生实现，且在更难的链上。
  2. **比特币"无发行人+无清算+自托管+期权/到期赎回"的格子真空着**（10101 死、conduition 仅论文）。
  3. **BitVM 2/3 能模拟 CSFS 但整套引入委员会/挑战者/operator/窗口**，背叛 no-issuer/单边——这正是项目早先把 BitVM 架构（ADR-0018/ADR-007）归档、转向 DLC 模型的原因。可拿走的只是最小原语。
  4. **今天可搭的 free-option 缓解**：timelock-授权过期 / 预言机 adaptor 短时效 / 最小乐观罚没闸——各自信任代价远小于"发行人"。

## 7. 当前战略落点（待 Jeff 拍板）

- **不去 Ethereum**（丢护城河进拥挤场）。
- **真正的问号是需求侧 + 执行耐力，不是"有没有价值"。** 10101 死于卖不动是市场风险，应优先回答。
- **下一步候选**：(A) 把 timelock-授权过期画成一页设计 + signet PoC；(B) 核实 conduition 重合度 + ADR-0018 是否还作数（已确认存在于 archive）；(C) 先做需求侧尽调（谁会买无发行人无清算的 BTC 期权美元）。

---

## 原始记录

本日志是 curated 实录。**逐字对话的完整原始记录**在本次会话 transcript（JSONL）：
`/Users/jeff/.claude/projects/-Users-jeff-Workplace-SatsUSD/91dff082-29ac-4919-b0d2-d3b1ff9a550d.jsonl`
（注：早期部分已被上下文压缩；JSONL 是未压缩的事实源。如需把它导出/复制进仓库留档，可另行处理。）

11-agent 证伪辩论的完整输出（B′ 裁决全文）：
`/Users/jeff/.claude/projects/-Users-jeff-Workplace-SatsUSD/91dff082-29ac-4919-b0d2-d3b1ff9a550d/tasks/wfokdnp2y.output`
