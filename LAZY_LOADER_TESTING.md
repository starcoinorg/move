# Lazy Loader 测试手册

## 背景与目标

`move` 的 lazy loader 已经移植完成。测试目标不是单独验证某个 API，而是同时确认两条执行路径都稳定：

- 默认 eager 路径兼容现有行为。
- 开启 lazy loading 后，已部署合约和新发布合约都能正确运行。
- 在局部触达依赖图的冷启动场景下，lazy loading 的首次验证时延优于 eager loading。

本手册把测试分成四类：

- 兼容性基线
- 已部署合约回归
- 新合约发布与执行
- 性能验证

性能验收指标固定为“验证时延”。本手册不把吞吐、gas、内存占用纳入正式验收指标。

## 测试开关与运行入口

lazy loader 的公开测试入口只有两类：

- `VMConfig.enable_lazy_loading`
- transactional test 命令中的 `--lazy-loading`

默认配置是 eager：

```rust
let config = VMConfig {
    enable_lazy_loading: false,
    ..Default::default()
};
```

transactional tests 中可以按命令粒度切换 lazy loading：

```text
//# publish --lazy-loading
//# run --lazy-loading
```

建议使用下面这组命令作为统一入口：

```bash
cargo test -p move-vm-runtime -p move-vm-test-utils -p move-vm-integration-tests -p move-unit-test
cargo test -p move-vm-transactional-tests
cargo test -p move-vm-integration-tests loader_tests
```

如果只想验证 lazy-loading transactional 用例，可以进一步限定：

```bash
cargo test -p move-vm-transactional-tests lazy_loading/
```

## 测试合约选型原则

测试合约按依赖图结构和运行时触达模式来选，不按业务语义来选。lazy loader 的收益和风险都来自“依赖图什么时候被真正走到”，所以合约必须覆盖下面这些形状和行为：

- 深依赖链
- DAG / 扇出依赖
- friend 关系
- 循环依赖 / 循环 friendship
- 泛型结构体或泛型函数
- layout / type tag 查询
- native / metadata 路径
- 局部触达依赖图
- 全闭包触达对照场景

选型时遵守两个原则：

1. 先复用仓库里已有且长期稳定的测试合约，保证兼容性基线可信。
2. 只有在现有用例无法体现 lazy 优势时，才补 synthetic contracts 做性能验证。

## 推荐测试合约清单

### 本仓库现有用例

| 文件 | 主要用途 | 适用测试 |
| --- | --- | --- |
| [language/move-vm/integration-tests/src/tests/loader_tests_modules.move](/Users/simon/starcoin-projects/move/language/move-vm/integration-tests/src/tests/loader_tests_modules.move) | 多模块依赖、局部触达、layout 查询 | 已部署合约回归、性能样例参考 |
| [language/move-vm/integration-tests/src/tests/loader_tests.rs](/Users/simon/starcoin-projects/move/language/move-vm/integration-tests/src/tests/loader_tests.rs) | `load`、`lazy_load`、并发加载、`get_type_layout` | eager/lazy 执行对照 |
| [language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use.mvir) | 新模块发布后立即执行 | 新合约发布 |
| [language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use_2.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use_2.mvir) | 发布后立即执行，覆盖另一组接口形状 | 新合约发布 |
| [language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use_3.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/publish_module_and_use_3.mvir) | 发布后立即执行，补不同返回值/调用路径 | 新合约发布 |
| [language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_fn.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_fn.mvir) | 函数循环依赖 | eager/lazy 语义差异 |
| [language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_friend.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_friend.mvir) | friend 环路 | eager/lazy 语义差异 |
| [language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_struct.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/module_publishing/republish_module_cyclic_dependencies_struct.mvir) | 结构体层面的循环依赖 | eager/lazy 语义差异 |
| [language/move-vm/transactional-tests/tests/lazy_loading/cyclic_dependency_publish_and_run.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/lazy_loading/cyclic_dependency_publish_and_run.mvir) | lazy 下允许发布，运行时报错 | lazy 专项 |
| [language/move-vm/transactional-tests/tests/lazy_loading/cyclic_friend_publish_and_run.mvir](/Users/simon/starcoin-projects/move/language/move-vm/transactional-tests/tests/lazy_loading/cyclic_friend_publish_and_run.mvir) | lazy 下 friend 环路可发布，运行时报错 | lazy 专项 |
| [language/tools/move-unit-test/tests/test_sources/signer_args.move](/Users/simon/starcoin-projects/move/language/tools/move-unit-test/tests/test_sources/signer_args.move) | 参数绑定与入口调用兼容性 | 兼容性基线 |
| [language/tools/move-unit-test/tests/test_sources/native_abort.move](/Users/simon/starcoin-projects/move/language/tools/move-unit-test/tests/test_sources/native_abort.move) | native 错误路径 | 兼容性基线 |
| [language/tools/move-unit-test/tests/test_sources/storage_test.move](/Users/simon/starcoin-projects/move/language/tools/move-unit-test/tests/test_sources/storage_test.move) | 资源读写与状态变化 | 兼容性基线 |
| [language/tools/move-unit-test/tests/test_sources/missing_data.move](/Users/simon/starcoin-projects/move/language/tools/move-unit-test/tests/test_sources/missing_data.move) | 缺失依赖/数据异常路径 | 兼容性基线 |
| [language/tools/move-unit-test/tests/test_sources/proposal_test.move](/Users/simon/starcoin-projects/move/language/tools/move-unit-test/tests/test_sources/proposal_test.move) | 跨模块调用与状态演化 | 兼容性基线 |

### Aptos 参考来源

下面这些文件适合参考“还可以补什么图形和断言”，但不是本仓库测试的执行前置：

- `aptos-core/third_party/move/move-vm/integration-tests/src/tests/loader_tests.rs`
- `aptos-core/third_party/move/move-vm/integration-tests/src/tests/module_storage_tests.rs`
- `aptos-core/third_party/move/move-vm/integration-tests/src/tests/native_tests.rs`

它们最有价值的地方是：

- 如何构造树状和 DAG 依赖图
- 如何断言 deserialized/verified cache 的状态变化
- 如何覆盖 native `LoadModule` 和 metadata 路径

### Aptos 结构型测试的可移植性结论

这批结构型测试可以继续移植，而且其中一部分已经在本仓库落地。

已经具备对应覆盖的内容：

- `lazy_load` / `lazy_load_concurrent`
- `lazy_get_type_layout`
- `load_phantom_module`
- `load_with_extra_ability`
- `deep_dependency_list_ok_*`
- `deep_friend_list_ok_*`

这些用例已经存在于 [language/move-vm/integration-tests/src/tests/loader_tests.rs](/Users/simon/starcoin-projects/move/language/move-vm/integration-tests/src/tests/loader_tests.rs)，说明 Aptos `loader_tests.rs` 中最核心的结构型样例已经完成了主体移植。

仍然适合继续移植的内容有两类：

- `module_storage_tests.rs` 里的 cache-state 断言
  - 重点是 deserialized cache / verified cache 的状态变化
  - 重点是 tree / DAG traversal 到了哪里
  - 重点是 lazy 是否真的没有预加载整个依赖闭包
- 以 `deep_dependency_list`、`deep_friend_list`、tree / DAG traversal 为模板的性能专用包装
  - 这些结构型测试本身更偏正确性
  - 但非常适合改造成“首次验证时延”的性能合约

需要注意的限制：

- 当前有一部分深依赖报错类测试仍是 `#[ignore]`
- 原因不是 lazy loader 不支持，而是依赖检查已经移到 Move VM 外部
- 因此这部分测试如果继续移植，必须按当前实现语义重写断言，不能直接恢复 Aptos 的旧预期

## 测试矩阵与预期结果

| 类别 | 合约/用例 | lazy 开关 | 预期结果 | 关注指标 |
| --- | --- | --- | --- | --- |
| 兼容性基线 | `move-vm-runtime`、`move-vm-test-utils`、`move-vm-integration-tests`、`move-unit-test` 全量现有测试 | 关闭 | 与 lazy loader 移植前保持一致，无新增失败 | 回归数为 0 |
| 已部署合约回归 | `loader_tests_modules.move` + `loader_tests.rs` | 开启 | 先 publish 再 run 成功，并发加载成功，`get_type_layout` / `get_fully_annotated_type_layout` 成功 | 不预加载整个依赖闭包 |
| 新合约发布 | `publish_module_and_use*.mvir` | 开启与关闭都执行 | 发布后立即调用成功，eager/lazy 都保持功能正确 | 发布后首次执行成功 |
| 循环依赖差异 | `republish_module_cyclic_dependencies_{fn,friend,struct}.mvir` | eager 与 lazy 对照 | eager 下发布失败；lazy 下允许发布，但运行期报错或按 baseline 报等价错误 | 发布/执行阶段语义符合设计 |
| lazy 专项 | `cyclic_dependency_publish_and_run.mvir`、`cyclic_friend_publish_and_run.mvir` | 开启 | 发布成功，运行时报运行期错误 | 错误阶段与错误类型符合预期 |
| 用户视角兼容性 | `signer_args.move`、`native_abort.move`、`storage_test.move`、`missing_data.move`、`proposal_test.move` | 关闭 | 结果与现有 `.exp` / `.v2_exp` 一致 | 无行为回归 |
| 性能验证 | 以 `loader_tests_modules.move` 为起点，配合后续 synthetic contracts | eager 与 lazy 对照 | 在局部触达依赖图的冷启动场景下，lazy 首次验证时延优于 eager | 首次验证时延中位数 |

## 性能验证方法

性能测试只比较“首次验证时延”。建议按下面的方法执行：

1. 使用冷缓存场景。
2. 每组用例至少跑 3 次，取中位数。
3. 同时记录 eager 和 lazy 的绝对时间与比值。
4. 把“局部触达依赖图”和“全闭包触达依赖图”分开看。

验收标准固定如下：

- 在局部触达依赖图的冷启动场景下，lazy 应明显优于 eager。
- 在全闭包都会被触达的场景下，不要求 lazy 一定更快。
- 如果热缓存场景差异不明显，不算失败。

当前仓库里可直接复用的起点是 `loader_tests_modules.move`，但它更适合做“现象验证”和“初步对照”。如果要做稳定的性能回归比较，应该再补两组 synthetic contracts。

## 建议补充的 synthetic contracts

这两类合约只服务于性能验证，不作为功能回归主线：

### 深链依赖

构造 `A -> B -> C -> ... -> N` 的依赖链，但单次运行只真正触发链尾少量定义。目标是放大 eager 的传递闭包预加载成本。

### 大 DAG / 扇出依赖

构造一个主模块依赖多个模块，但某次执行只会走其中一条路径。目标是验证 lazy 在“大依赖图但局部触达”场景下的收益。

这两类 synthetic contracts 的设计要求：

- 依赖结构固定且可重复构造
- 执行入口单一，避免 benchmark 受业务逻辑噪音影响
- eager 和 lazy 使用同一组合约与同一调用参数

## 推荐执行顺序

建议按下面顺序跑，避免性能测试掺入功能回归：

1. 跑默认 eager 基线，确认兼容性。
2. 跑 `loader_tests.rs` 中的 lazy 用例，确认已部署模块和 layout 路径正常。
3. 跑 `module_publishing` 与 `lazy_loading` transactional tests，确认发布/执行语义。
4. 最后单独跑性能对照。

对应命令如下：

```bash
cargo test -p move-vm-runtime -p move-vm-test-utils -p move-vm-integration-tests -p move-unit-test
cargo test -p move-vm-integration-tests loader_tests
cargo test -p move-vm-transactional-tests
cargo test -p move-vm-transactional-tests lazy_loading/
```
