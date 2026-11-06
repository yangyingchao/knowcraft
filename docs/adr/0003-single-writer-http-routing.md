# ADR 0003：写者仲裁——唯一写者 + HTTP 路由

- 日期：2026-11-02
- 状态：已接受

## 背景

knowcraft 有四个写入口（GUI/HTTP/MCP/CLI）共享 knowcraft-core。SQLite（WAL 单写者）与 tantivy 索引要求串行写。

## 决策

GUI 进程为唯一写者，持有 SQLite 与 tantivy 写权限。GUI 运行时，CLI 的写请求经 HTTP 路由到 GUI 进程执行；无 GUI 运行时 CLI 自持锁写（降级运行）。watcher 与写操作同进程时抑制自反馈。

## 理由

- 单一写入路径消除跨进程写竞争，比"多进程持锁自写"更可预期
- 与单进程内嵌服务的架构一致（GUI 即服务宿主）
- SQLite 锁无法保护 tantivy 索引文件，唯一写者从根上消除此问题

## 后果

- 无 GUI 时 CLI 写功能受限为降级模式（自身持锁）
- CLI 需感知 HTTP 服务可用性并路由写请求
