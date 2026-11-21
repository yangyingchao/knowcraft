# Changelog

## [Unreleased]

### Added
### Changed
### Fixed


## [v0.3.1] - 2026-11-21

### Changed
- 文档与 CLI 帮助的 doc-server 表述改为对接 edoc-tools
- 设计文档按 edoc-tools 现状重写，命令表按实现列出
- AGENTS.md 补上 CLI 分组顺序与数据目录的实际结构
- README 功能描述追平实现，致谢表补上 edoc-tools


## [v0.3.0] - 2026-11-20

### Added
- 新增 kc doc-server 子命令接管 pdf-tools 后端 (228702c)
- 新增 kc init 子命令生成 shell 补全脚本 (9bb539a)
- 添加标签支持分隔符批量输入与实时预览 (4d8ee94)

### Changed
- 清理 clippy 警告并移除未接线的更新通道 (d4c239b)
- kc launch 改为 run 并支持 --ai，修复移动对话框库外空白 (826afda)

## [v0.2.2] - 2026-11-16

### Added
- 新建按钮改为菜单，增加新建日期文件夹 (d6d7c99)
- 摘要弹窗支持 Esc 与关闭按钮 (d645160)

### Changed
- 发布流程从 CHANGELOG 抽取 release note (b07f012)

## [v0.2.1] - 2026-11-15

### Added
- 新增 Hermes 文档摘要原型，预览抽屉可生成并查看摘要 (3ee014d)
- 摘要改用 TextView 显示，可选中复制并导出为笔记 (7e2896c)

### Changed
- 统一 settings.toml：core 字段留顶层，GUI 偏好移入 [gui] (0c71eaa)
- 库外文件删除改走系统回收站 (d9718de)

[Unreleased]: https://github.com/yangyingchao/knowcraft/compare/v0.3.1...HEAD
[v0.3.1]: https://github.com/yangyingchao/knowcraft/compare/v0.3.0...v0.3.1
[v0.3.0]: https://github.com/yangyingchao/knowcraft/compare/v0.2.2...v0.3.0
[v0.2.2]: https://github.com/yangyingchao/knowcraft/compare/v0.2.1...v0.2.2
[v0.2.1]: https://github.com/yangyingchao/knowcraft/compare/v0.2.0...v0.2.1
