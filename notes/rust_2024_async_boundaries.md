# RUST 2024 ASYNC TRAIT BOUNDARIES

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-23
- **Promoted To:** `../docs/RUST_2024_ENGINEERING.md`, `../docs/RUNTIME_SANDBOX.md`

## 1. Context & Problem

The runtime uses `#[async_trait]` for both statically dispatched internal traits and object-safe
extension points. Removing the dependency requires different treatment: native trait async functions
are simpler and avoid boxing for static dispatch, while `Arc<dyn ModelStreamProvider>` and sandbox
handles still require object safety. Integration tests define custom providers, so closing every trait
into an enum would break the existing extension contract.

## 2. Rejected Options & Why

- **Keep `async-trait` everywhere:** rejected because static dispatch pays an avoidable boxed-future
  allocation and the workspace has native Rust 2024 support.
- **Use native `async fn` everywhere:** rejected for `dyn Trait`; native trait async methods are not
  object safe and would break runtime provider substitution.
- **Close all providers into enums:** rejected for model streaming because integration tests and the
  runtime service intentionally accept custom `ModelStreamProvider` implementations. An enum would
  move the extension boundary rather than improve it.

## 3. Chosen Approach & Trade-offs

Use native trait async functions for `InvalidationSubscriber` and `SessionEventLog`. Use explicit
`Pin<Box<dyn Future<...> + Send + '_>>` only for object-safe `ModelStreamProvider`, `SandboxProvider`
and `Sandbox` methods. Implementations return `Box::pin(async move { ... })`, preserving signatures,
mock providers and runtime behavior while deleting the `async-trait` dependency.

The trade-off is retained allocation at the genuinely dynamic boundaries. Static paths no longer box,
and the dynamic allocation is explicit rather than hidden by a macro. Large patch variants are boxed
separately to reduce enum stack size without changing their JSON representation.
