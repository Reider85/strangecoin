# Strangecoin UI — 简体中文翻译

## 导航 / 外壳
app-title = Strangecoin
nav-wallet = 钱包
nav-dashboard = 节点面板
nav-transactions = 交易
nav-agents = 智能体
nav-settings = 设置
nav-back = 返回
nav-home = 首页

## 钱包
wallet-title = 钱包
wallet-send-tab = 发送
wallet-history-tab = 历史记录
wallet-contracts-tab = 合约
wallet-balance = 余额
wallet-address = 地址
wallet-send-recipient = 收款地址
wallet-send-amount = 金额
wallet-send-fee = 手续费
wallet-send-submit = 发送
wallet-send-success = 交易已提交：{ $txid }
wallet-send-error-invalid-address = 无效的 bech32 地址
wallet-send-error-insufficient-balance = 余额不足
wallet-send-form-title = 发送交易
wallet-recent-transactions = 最近交易
wallet-deployed-contracts = 已部署合约
wallet-view-details = 查看详情
wallet-view-abi = 查看 ABI
wallet-interact = 交互
wallet-empty-history = 暂无交易
wallet-empty-contracts = 暂无已部署合约
wallet-send-batch = 批量发送
wallet-send-batch-unavailable = 批量发送将在 AI-P04 后可用
wallet-history-filter-all = 全部
wallet-history-filter-sent = 已发送
wallet-history-filter-received = 已接收
wallet-history-filter-failed = 失败
wallet-history-col-txid = TxID
wallet-history-col-type = 类型
wallet-history-col-from = 来自
wallet-history-col-to = 发送至
wallet-history-col-amount = 金额
wallet-history-col-fee = 手续费
wallet-history-col-status = 状态
wallet-history-col-height = 高度
wallet-history-prev = 上一页
wallet-history-next = 下一页
wallet-contracts-read = 读取
wallet-contracts-write = 写入

## 节点面板
dashboard-title = 节点面板
dashboard-peers = 节点
dashboard-sync = 同步状态
dashboard-mempool = 内存池
dashboard-blocks = 最新区块
dashboard-total-balance = 总余额
dashboard-pending-transactions = 待处理交易
dashboard-network-hashrate = 网络算力
dashboard-active-agents = 活跃智能体
dashboard-recent-transactions = 最近交易
dashboard-tip-height = 链尖高度
dashboard-sync-percent = 已同步 { $percent }%
dashboard-connected-peers = 已连接 { $count } 个节点
dashboard-uptime = 运行时间
dashboard-version = 版本

## 交易详情
tx-detail-title = 交易
tx-detail-receipt = 回执
tx-detail-witness = 见证证明
tx-detail-events = 事件
tx-detail-overview = 概览
tx-detail-status = 状态
tx-detail-block = 区块
tx-detail-from = 发送方
tx-detail-to = 接收方
tx-detail-amount = 金额
tx-detail-fee = 手续费
tx-detail-gas-used = 已用 Gas
tx-detail-timestamp = 时间戳
tx-detail-input-data = 输入数据
tx-detail-raw-data = 原始交易数据
tx-detail-verify = 验证
tx-detail-load = 加载交易
tx-detail-not-found = 未找到交易

## 智能体活动
agent-activity-title = 智能体活动
agent-activity-filter = 筛选
agent-activity-agents = 智能体
agent-activity-activities = 活动
agent-activity-total-agents = 智能体总数
agent-activity-online-agents = 在线
agent-activity-offline-agents = 离线
agent-activity-error-agents = 错误
agent-activity-refresh = 刷新活动
agent-activity-export = 导出活动
agent-activity-view-details = 查看智能体详情
agent-activity-empty = 暂无智能体活动

## 设置
settings-title = 设置
settings-general = 常规
settings-network = 网络
settings-security = 安全
settings-appearance = 外观
settings-about = 关于
settings-peers = 节点
settings-keystore = 密钥库
settings-config = 配置
settings-mcp = MCP 服务器
settings-acp = ACP 传输
settings-save = 保存设置
settings-reset = 恢复默认值
settings-language = 语言
settings-currency = 货币
settings-theme = 主题
settings-font-size = 字体大小
settings-data-directory = 数据目录

## 主题
theme-light = 浅色
theme-dark = 深色
theme-system = 跟随系统

## 按钮 / 操作
button-ok = 确定
button-cancel = 取消
button-save = 保存
button-delete = 删除
button-add = 添加
button-edit = 编辑
button-close = 关闭
button-confirm = 确认
button-copy = 复制
button-refresh = 刷新
button-search = 搜索
button-filter = 筛选
button-export = 导出
button-import = 导入
button-login = 登录
button-register = 注册
button-send = 发送
button-retry = 重试
button-apply = 应用

## 状态 / 通用
loading = 加载中…
error = 错误
success = 成功
warning = 警告
info = 信息
status-online = 在线
status-offline = 离线
status-pending = 待处理
status-confirmed = 已确认
status-failed = 已失败
empty-state = 暂无内容
required-field = 此字段为必填项
invalid-format = 格式无效
unknown-error = 发生未知错误
confirm-title = 确定要继续吗？
unsaved-changes = 有未保存的更改
search-placeholder = 搜索…
filter-all = 全部
theme-switched = 主题已切换为 { $theme }
language-switched = 语言已切换为 { $language }

## UI-P04: 节点面板
dashboard-current-height = 当前高度
dashboard-sync-state = 同步状态
dashboard-headers-queue = 区块头队列
dashboard-blocks-queue = 区块队列
dashboard-score-healthy = 正常
dashboard-score-warning = 警告
dashboard-score-banned = 已封禁
dashboard-no-peers = 未连接节点
dashboard-peer-id = 节点 ID
dashboard-peer-addr = 地址
dashboard-peer-latency = 延迟
dashboard-peer-score = 评分
dashboard-peer-direction = 方向
dashboard-peer-capabilities = 能力
dashboard-total-fee = 总手续费
dashboard-no-mempool = 内存池为空
dashboard-mempool-txid = 交易 ID
dashboard-mempool-from = 发送方
dashboard-mempool-to = 接收方
dashboard-mempool-amount = 金额
dashboard-mempool-fee = 手续费
dashboard-mempool-gas = Gas
dashboard-mempool-rbf = RBF 冲突
dashboard-no-blocks = 暂无区块
dashboard-block-height = 高度
dashboard-block-hash = 哈希
dashboard-block-txs = 交易数
dashboard-block-gas = 已用 Gas
dashboard-block-miner = 矿工
dashboard-block-timestamp = 时间戳
dashboard-block-tx-list = 交易列表
dashboard-block-detail = 区块详情

## UI-P04: 交易详情
tx-detail-txid = 交易 ID
tx-detail-kind = 类型
tx-detail-status-pending = 待处理
tx-detail-copy = 复制
tx-detail-gas-limit = Gas 上限
tx-detail-revert = 回滚原因
tx-detail-events-empty = 无事件
tx-detail-receipt-unavailable = 回执不可用（仅保留链尖区块）
tx-detail-verified = 已验证
tx-detail-verify-failed = 验证失败
tx-detail-witness-unavailable = 点击「验证」加载证明
tx-detail-witness-account = 账户证明
tx-detail-witness-storage = 存储证明
tx-detail-witness-raw = 见证线格式证明
tx-detail-event-topics = 主题
tx-detail-event-data = 数据
tx-detail-no-selection = 请选择交易
tx-detail-loading = 正在加载交易详情…
