# 第三方组件与资源

本次公开的 InkSay 桌面源码采用 GPL-3.0-only。以下组件保留原版权和许可证，不因主项目许可证而重新许可。

## 主要组件

| 组件 | 用途 | 上游许可证 / 声明 |
| --- | --- | --- |
| Tauri 2、Tauri 插件、tauri-nspanel | 桌面运行时、系统功能和 macOS 浮窗 | MIT / Apache-2.0，具体以固定版本为准 |
| React、Radix UI、Recharts、OpenCC JS | 桌面界面、图表、简繁转换 | MIT；其中第三方图表工具还包含自己的子组件声明 |
| Lucide | 界面图标 | ISC |
| zhconv 0.4.2 | Rust 中文转换 | GPL-2.0-or-later；使用其 GPL v3 许可选项，与主项目 GPL-3.0-only 配合 |
| jieba-rs、rubato、hound | 分词、音频采样转换、WAV | MIT / Apache-2.0，见清单 |
| flacenc 0.5.1 | FLAC 无损压缩 | Apache-2.0；[许可原文](src-tauri/resources/FLACENC-LICENSE) |
| whisper.cpp / ggml | 本地识别和 VAD 辅助进程 | 上游固定 revision `f24588a272ae8e23280d9c220536437164e6ed28` 为 MIT，本仓库自有修改按 GPL-3.0-only 提供；[许可原文](src-tauri/resources/WHISPER-LICENSE) |
| Silero VAD v6.2.0 的 ggml 模型 | 本地语音检测 | MIT，Copyright Silero Team；[许可原文](src-tauri/resources/SILERO-LICENSE) |
| `asr-test.wav` | 用户主动连接测试和离线 VAD 测试 | whisper.cpp 中 JFK 演说公开样本的前四秒；来源和校验说明见[资源文档](src-tauri/resources/README.md) |

## 完整清单与文本

- [固定版本依赖清单](docs/licenses/dependencies.json)：Cargo 与 npm 的锁文件解析结果，包含其他平台及开发依赖，不表示全部会编入每个平台的二进制。
- [依赖许可原文与版权通知](src-tauri/resources/THIRD-PARTY-LICENSES.txt)：收集 macOS ARM64 / Windows x64 MSVC Rust 依赖及生成时本机安装的 npm 包；相同文本去重，保留组件对应关系及来源。
- [Rust 补充上游声明](docs/licenses/upstream-notices.json)、[npm 补充声明](docs/licenses/npm-upstream-notices.json)：用于补齐发布包中未附许可文件的组件，来源固定到上游 revision 或固定版本发布包。少数上游仅在 Cargo 元数据或 README 中声明许可，清单如实保留这些声明，不补造版权年份或独立许可文件。

锁文件改变后运行 `npm run licenses:refresh`，审查新增或变更的条款并提交结果。生成器缺少必要许可文本时会报错，不猜测版权人或用主项目许可证替换第三方条款。

## 下载模型与外部服务

用户后续下载的 Whisper 模型不随源码包或应用安装包内置。各模型的来源由模型配置代码指定，使用及再分发需要遵守对应上游条款。

云端模型服务由使用者自行选择和配置，其 API、商标、计费与隐私条款不属于本项目的 GPL 授权。

## 源码提供

分发 InkSay 二进制时，应同时提供该发行对应的完整源码、修改、构建脚本及必要的第三方对应源码。只有依赖版本清单或指向一个可变分支的链接，不应当作已完成对应源码交付。公开二进制的准备步骤见 [发布指南](docs/RELEASE.md)。
