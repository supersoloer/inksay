# 开发与构建

## 环境

- Node.js 22 或更新版本；`.nvmrc` 当前为 22。
- Rust stable（已验证 1.89.0）、Git、CMake。
- macOS：Xcode Command Line Tools。
- Windows：Visual Studio 的 C++ 桌面开发工具及 Windows SDK、WebView2 Runtime。在 Developer PowerShell 中执行构建。

应用源码、模型参数和运行数据彼此独立。全新检出不会自动获取个人配置或 API Key。

## 从源码运行

使用 Git 克隆仓库后，在仓库根目录执行：

```sh
npm ci
npm run tauri dev
```

`beforeDevCommand` 会构建本地语音辅助进程并启动 Vite。首次构建从 whisper.cpp 上游获取固定版本源码，应用本仓库的补丁，校验内置 VAD 模型并编译 sidecar。开发过程不需要云端 API Key；实际云端功能由应用设置中的配置决定。

本地前端预览 `npm run dev` 不提供 Tauri IPC，不能据此验收录音或系统写入。

### 下载源码归档时

源码归档不包含 `.git`。本项目的构建元数据需要 Git 工作区；解压后可先初始化本机仓库，再执行上述命令：

```sh
git init
git add .
git -c user.name='Local Builder' -c user.email='builder@localhost' commit -m 'Import InkSay source'
```

因此归档构建的本机 commit 会与原发布 commit 不同；原发布信息可在归档附带的 source manifest 中核对。正式发行应使用对应 Tag 的 Git 检出。

## 测试

```sh
npm run check:public
npm run build
npm run test:statistics
npm run build:whisper
npm run smoke:whisper
cargo test --manifest-path src-tauri/Cargo.toml --locked --release --lib
```

本地 VAD 测试使用内置公开样本，不连接云端：

```sh
python3 scripts/test-vad-gate.py --speech src-tauri/resources/asr-test.wav
```

Windows 使用 `python`。CI 的 macOS / Windows 作业运行相应构建与测试，不调用个人模型服务。云端协议测试连接本机 HTTP 服务，不能替代真实服务或设备验收。

Windows 的交叉编译检查可使用 `x86_64-pc-windows-gnu` 工具链，结果仅证明该目标编译通过。发布构建优先使用 Windows 的原生 MSVC 环境。

## macOS 打包

开发者本机预览：

```sh
INKSAY_CODESIGN_IDENTITY='<Apple Development identity>' npm run build:preview:macos
```

正式包要求已提交、干净的源码树与一致的版本：

```sh
npm run version:set -- 0.1.0
# 审查并提交改动后执行
INKSAY_CODESIGN_IDENTITY='<signing identity>' npm run build:production:macos
npm run start:macos
```

包位于 `build/releases/macos/InkSay_V<version>.<build>.zip`，旁边的 manifest 记录 commit、架构、构建号和 SHA-256。

脚本使用签名身份并验证音频权限，但没有执行 Apple 公证。Apple Development 签名和本机可运行不代表可以无提示公开分发。公开 macOS 二进制需要准备适当的 Developer ID 签名、公证及装订流程，发布前按 [RELEASE.md](RELEASE.md) 检查。

## Windows 打包

```powershell
npm ci
npm run build:production:windows
```

产物为 `build/releases/windows/InkSay_V<version>.<build>.zip` 及 manifest。当前脚本不执行 Windows Authenticode 签名。CI 验证 NSIS 构建，但默认不上传或发布二进制。正式发行还需提供匹配的 GPL 对应源码，不能将测试产物描述为已签名发行。

## 许可证更新

锁文件变更后执行：

```sh
npm ci
npm run licenses:refresh
npm run check:public
```

生成器按 Cargo 与 npm 的固定版本记录依赖。Rust 发布目标为 macOS ARM64 与 Windows x64/MSVC，完整锁文件清单也保留；并收集本机已安装 npm 包中的许可文本。缺少许可文件的依赖需要先补齐有来源的声明，不自动猜测许可证。

## 缓存与体积

停止构建后执行：

```sh
npm run clean:build -- --dry-run
npm run clean:build
```

保留源码、锁文件、模型、发布归档及 release/bundle 内的应用，清理 Rust 编译缓存。清理后的第一次构建会重新编译依赖。开发 profile 减少调试符号和增量缓存；需要源码级调试时可临时设置 `CARGO_PROFILE_DEV_DEBUG=1`。

历史资料位于 `docs/archive/`、`docs/reference/` 和内部调研文件中，不应将其当成当前构建步骤。首次公开发布使用经过检查的源码包；个人档案和生成产物不进入公开源码归档。
