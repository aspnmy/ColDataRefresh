# GNU工具链下载方式更换指南

## 当前配置情况

- 当前已安装GNU工具链：`stable-x86_64-pc-windows-gnu`（默认活跃）
- 当前使用的镜像源：清华大学镜像源
  - `RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup`
  - `RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup`

## 更换下载方式的方法

### 方法一：更换为其他镜像源

#### 1. 中国科学技术大学镜像源

```powershell
# 设置环境变量
$env:RUSTUP_DIST_SERVER="https://mirrors.ustc.edu.cn/rust-static"
$env:RUSTUP_UPDATE_ROOT="https://mirrors.ustc.edu.cn/rust-static/rustup"

# 安装或更新GNU工具链
rustup install stable-x86_64-pc-windows-gnu
```

#### 2. 上海交通大学镜像源

```powershell
# 设置环境变量
$env:RUSTUP_DIST_SERVER="https://mirrors.sjtug.sjtu.edu.cn/rust-static"
$env:RUSTUP_UPDATE_ROOT="https://mirrors.sjtug.sjtu.edu.cn/rust-static/rustup"

# 安装或更新GNU工具链
rustup install stable-x86_64-pc-windows-gnu
```

#### 3. 字节跳动镜像源

```powershell
# 设置环境变量
$env:RUSTUP_DIST_SERVER="https://rsproxy.cn"
$env:RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup"

# 安装或更新GNU工具链
rustup install stable-x86_64-pc-windows-gnu
```

### 方法二：离线安装GNU工具链

#### 1. 下载离线安装包

从Rust官网或镜像源下载对应版本的GNU工具链离线安装包：

- 清华大学镜像源：https://mirrors.tuna.tsinghua.edu.cn/rustup/dist/
- 中国科学技术大学镜像源：https://mirrors.ustc.edu.cn/rust-static/dist/

查找格式为 `rust-stable-x86_64-pc-windows-gnu.tar.gz` 的文件下载。

#### 2. 安装离线包

```powershell
# 使用rustup安装离线包
rustup toolchain install stable-x86_64-pc-windows-gnu --file <path-to-downloaded-tar.gz>
```

### 方法三：使用MSYS2安装GNU工具链

#### 1. 安装MSYS2

从MSYS2官网下载并安装MSYS2：https://www.msys2.org/

#### 2. 安装GNU工具链

```bash
# 更新包数据库
pacman -Syu

# 安装GNU工具链
pacman -S mingw-w64-x86_64-toolchain
```

#### 3. 配置环境变量

将MSYS2的bin目录添加到系统环境变量PATH中，例如：
- `C:\msys64\mingw64\bin`
- `C:\msys64\usr\bin`

## 验证安装

```powershell
# 检查当前活跃的工具链
rustup show

# 验证GNU工具链是否正常工作
rustc --version --verbose
cargo --version --verbose
```

## 配置Cargo使用国内镜像源

在`%USERPROFILE%\.cargo\config.toml`文件中添加以下内容，配置Cargo使用国内镜像源：

```toml
[source.crates-io]
replace-with = 'rsproxy'

[source.rsproxy]
registry = "https://rsproxy.cn/crates.io-index"

[registries.rsproxy]
index = "https://rsproxy.cn/crates.io-index"

[net]
git-fetch-with-cli = true
```

## 切换默认工具链

如果需要将GNU工具链设置为默认工具链，可以使用以下命令：

```powershell
rustup default stable-x86_64-pc-windows-gnu
```

## 卸载旧工具链

如果需要卸载旧的MSVC工具链，可以使用以下命令：

```powershell
rustup uninstall stable-x86_64-pc-windows-msvc
```

## 注意事项

1. 更换镜像源后，建议清理缓存并重新安装依赖：
   ```powershell
   cargo clean
   cargo build
   ```

2. 离线安装包的版本必须与当前Rust版本匹配，否则可能会出现兼容性问题。

3. 使用MSYS2安装的GNU工具链可能需要额外配置环境变量，确保Rust能够正确找到工具链。

4. 更换工具链后，建议重新构建项目，确保所有依赖都能正确编译。