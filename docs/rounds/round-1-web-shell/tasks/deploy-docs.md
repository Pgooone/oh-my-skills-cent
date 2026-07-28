# 任务卡：deploy-docs（批次 4，与 build-packaging 并行）

> 设计依据：`../detailed-design.md` §6。**前置**：批次 2、3 已验收（产物形态已知）。

- [ ] 新建 `docs/deployment.md`（仓库 docs/ 根部，跨轮资产）：
  - 构建：`npm run web:build`（先前端后 Rust 的顺序说明）
  - 配置：`OMS_PORT`（默认 8477）、`OMS_DATA_DIR`（默认 `~/.oh-my-skills-cent`）
  - 运行时依赖：系统 `git`（skills.sh 更新检查走 `git clone`）
  - systemd unit 示例（User=、WorkingDirectory、ExecStart、Restart）
  - SSH 隧道访问：`ssh -L 8477:127.0.0.1:8477 user@server`，并解释为何只监听 localhost（ADR-0008 + D8）
  - WSL2 验证步骤
  - 故障排查：端口占用 / git 缺失 / 数据目录权限 / 非 localhost 拒绝启动是设计行为
- [ ] README 增补一节「Web 版（Linux 服务器）」指向 deployment.md（中英两版 README 都补）

**红线**：不改 CI；不 git commit。
