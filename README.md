<div align="center">
  <img src="assets/综测记录图标.svg" width="112" height="112" alt="综测记录图标">
  <h1>综测记录</h1>
  <p>在本机整理大学生综合测评记录、分数与证明材料。</p>

[![持续集成](https://github.com/GodBook/zongce-records/actions/workflows/ci.yml/badge.svg)](https://github.com/GodBook/zongce-records/actions/workflows/ci.yml)
[![最新版本](https://img.shields.io/github/v/release/GodBook/zongce-records?display_name=tag)](https://github.com/GodBook/zongce-records/releases/latest)
[![MIT](https://img.shields.io/badge/license-MIT-176b4d)](LICENSE)
</div>

“综测记录”是一款面向 Windows 10/11 x64 的中文单机软件。它把院级、校级、省级和国家级活动，与日期、分数、备注及证明材料保存在同一处，并提供筛选、统计、Excel 交换和完整备份。

> [!IMPORTANT]
> 记录、附件和备份默认只保存在你的电脑上。软件没有账号、云同步、遥测、广告或远程日志；断网不会影响日常使用。

## 主要功能

- 记录活动名称、类别、综测级别、日期、精确到两位小数的分数和备注。
- 将图片、PDF、Office 文件等证明材料复制到 SHA-256 内容寻址资料库；单条记录最多 20 份，每份不超过 200 MB。
- 按学年、日期、类别、级别和材料状态筛选，搜索活动、备注与材料名。
- 查看总分、记录数、附件数、待补材料数，以及级别、类别和月度趋势。
- 使用官方 Excel 模板批量导入；提交前预览新增、更新、疑似重复和逐行错误。
- 导出 Excel 明细与统计汇总，或生成含清单和附件目录的提交材料包。
- 创建 `.zcbak` 完整备份，支持校验后合并或可回滚替换恢复。
- 记录删除后进入回收站并保留 30 天；每天首次修改前保留最近 7 个内部恢复点。
- 通过 GitHub Releases 检查并安装经过 Tauri 签名验证的更新。

## 安装

从 [GitHub Releases](https://github.com/GodBook/zongce-records/releases/latest) 下载 Windows x64 NSIS 安装程序。安装范围为当前用户，不需要管理员权限。

> [!NOTE]
> 首个版本没有商业 Authenticode 证书，Windows SmartScreen 可能显示“未知发布者”。应用内更新包仍会使用编译进软件的公钥验证签名。

默认数据目录由 Windows 应用数据目录决定。可以在“设置 → 备份与存储”迁移到其他磁盘；软件会先复制并校验，成功后再切换位置。卸载软件不会主动删除用户数据。

## 备份与恢复

在“设置 → 备份与存储”创建 `.zcbak` 完整备份。备份包含 SQLite 一致性快照、清单和当前仍被引用的全部附件，可在另一台电脑上选择“合并恢复”或“替换恢复”：

- 合并恢复保留本地记录，同 UUID 冲突时默认保留本地版本。
- 替换恢复先在暂存目录完成格式、路径、大小和哈希校验，再切换数据目录并保留可回滚旧目录。
- 每天首次修改前生成的内部恢复点只防近期误操作，不能替代保存到其他磁盘的手动完整备份。

> [!WARNING]
> 本地数据库、附件和 `.zcbak` 备份均未加密。请把含个人证明材料的备份保存到可信位置，并使用 Windows 设备加密、BitLocker 或其他磁盘加密措施保护它。

## 隐私与更新

业务数据不会上传到 GitHub。软件仅在检查更新时访问 `github.com`、GitHub API 和 Release 下载地址；后台检查最多每 24 小时一次，也可以在设置页手动触发。下载的更新必须通过内置公钥验证，网络失败、文件损坏或签名伪造都会被拒绝，不影响本地记录功能。

更新元数据和安装包来自本仓库的公开 Release。请不要从不明网盘或第三方下载站安装修改版。

## 技术架构

```text
React + TypeScript
        │ 类型化 Tauri IPC
        ▼
Rust 模块化单体 ── SQLite（WAL、外键、迁移）
        ├────────── SHA-256 附件库 / 恢复点
        └────────── GitHub Releases（仅软件更新）
```

分数通过整数百分位累计，避免浮点误差；记录使用 UUID 和 `revision` 乐观锁；Excel 导入令牌与提交结果具备幂等性。备份恢复会拒绝路径穿越、异常压缩比、清单外文件和哈希不匹配内容。

更完整的设计说明见 [架构说明](docs/架构说明.md)，维护者发布流程见 [发布指南](docs/发布指南.md)。

## 本地开发

需要 Node.js 24.18.0、pnpm 11.20.0、Rust 1.97.1 和 Windows WebView2 开发环境。版本分别固定在 GitHub Actions、`package.json` 和 `rust-toolchain.toml`。

```powershell
pnpm install --frozen-lockfile
pnpm dev
```

启动桌面应用：

```powershell
pnpm tauri dev
```

执行发布前检查：

```powershell
pnpm check
pnpm version:check
pnpm build
pnpm test:e2e
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked performance_5000 -- --ignored --nocapture
node node_modules/@tauri-apps/cli/tauri.js build --ci --config src-tauri/tauri.ci.conf.json -- --locked
```

无签名的本地 NSIS 安装器输出到 `src-tauri/target/release/bundle/nsis/`。正式 Release 由 GitHub Actions 使用受保护的更新签名密钥生成更新包、`.sig` 和 `latest.json`；本地普通构建不会读取签名私钥。

浏览器开发模式使用 `localStorage` 中的演示数据，不会访问桌面应用的 SQLite 或附件库。

## 已知限制

`v0.1.x` 仅支持 Windows 10/11 x64 单机使用，不包含多用户、云同步、自动计分规则、审核流、OCR、移动端、深色主题、便携版或数据加密。首版没有商业 Authenticode 证书，且依赖系统 WebView2；安装程序会按 Tauri 的默认策略处理缺失的 WebView2 运行时。

项目以 [MIT 许可证](LICENSE) 开源。
