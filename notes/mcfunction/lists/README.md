# Community List-Implementation Survey

This directory records implementation patterns found in real Minecraft Java data
packs and public mcfunction experiments. It complements the basal command algebra
in [`../list-operation-algebra.md`](../list-operation-algebra.md): that note says
what vanilla can do; these notes say how other authors have composed those
primitives into usable collection libraries.

The target remains Java 26.2. Community code is treated as design evidence, not as
text to copy. Some sources target older Minecraft versions, and one surveyed
library has no repository license. Each note therefore separates the transferable
idea from version-specific syntax and implementation costs.

## Surveyed implementations

| Source | Snapshot inspected | Why it matters |
| --- | --- | --- |
| [Bookshelf `bs.collection`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection) | v4.1.0 commit `c719575`, 2026-06-16, Minecraft 26.2 | Current, tested functional collection API: map/filter/folds/scans/search/set operations/windows/zip/flatten |
| [stdmodulesystem collections](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections) | commit `d85c51c`, 2025-01-05, Minecraft 1.21; repository archived 2025-06-28 | Broad mutable collection suite, reference-path ABI, callback stack, ordered map links, multimaps |
| [Arcensoth list iteration](https://gist.github.com/Arcensoth/8a8a63df6985f289430a98fa4b5cbe99) | revision active 2024-05-25; originally 1.14 | Minimal two-list destructive traversal |
| [intsuc two-list zipper](https://gist.github.com/intsuc/ab0be47a4c51798ca2c108a7eef431d2) | revision active 2020-02-25 | Constant-structure cursor movement using two tail stacks |
| [intsuc growable ring queue](https://gist.github.com/intsuc/dcc55f14bd04f68b994b15c27305e1aa) | revision active 2025-04-08 | Queue avoiding repeated front removal, with ring and overflow regions |
| [intsuc bucket sort](https://gist.github.com/intsuc/f7a9aa18cea4bb35491928af04e827d9) | revision active 2019-11-24 | Bounded-domain sorting by direct bucket placement |
| [intsuc list-mapped trie](https://gist.github.com/intsuc/0901df9d487f7829d97491613a12d351) | revision active 2023-04-20 | Fixed-depth integer-key trie encoded entirely as nested lists |

Bookshelf is MPL-2.0. The stdmodulesystem snapshot does not contain a repository
license, so only its abstract patterns should be carried forward unless permission
is established. Gists are cited for study; this notebook paraphrases their designs
and does not reproduce their source.

## Findings by topic

- [Callbacks and re-entrant frames](callbacks-and-frames.md): a stable lambda ABI,
  frame ownership, nested collection calls, and the limits of shared scratch state.
- [Traversal direction and structural cost](traversal-direction-and-cost.md): why
  tail work is the default, when dynamic indexing is better, and why many elegant
  loops are accidentally quadratic.
- [Exact equality, matching, membership, and count](equality-membership-and-count.md):
  the no-op mutation equality oracle, native exact counting, and the boundary
  between NBT matching and equality.
- [Folds, scans, predicates, and search](folds-scans-and-search.md): accumulator
  protocols, order, index semantics, and command-level short circuiting.
- [Map, filter, flatten, partition, slices, windows, and zip](collection-shaping.md):
  the common collection transformations and how to fuse them.
- [Distinct and set-like list operations](set-like-operations.md): stable-order set
  semantics, their quadratic baseline, and when a keyed representation is needed.
- [Stacks, queues, deques, and zippers](stacks-queues-and-zippers.md): practical
  representations that keep mutation at list tails.
- [References, dictionaries, ordered maps, and multimaps](references-and-ordered-maps.md):
  command-path references, dynamic compound keys, linked insertion order, and
  dictionary representation choices.
- [Buckets and tries](specialized-indexes.md): bounded-domain specialization and
  generated paths as alternatives to a generic scan.

## What MDL should adopt

1. Give higher-order operations one typed callback ABI, including value, index,
   accumulator where relevant, and an explicit return/result channel.
2. Allocate an owned frame for every invocation so callbacks may nest safely.
3. Preserve semantic order independently from physical traversal direction.
4. Make tail traversal, tail removal, and tail append the default lowering.
5. Treat `return`, command success, and command result as first-class control/data
   channels; several powerful collection primitives are encoded there.
6. Fuse pipelines such as map+flatten, filter+map, and find+predicate when callbacks
   permit it, avoiding intermediate deep copies.
7. Specialize by known size, key domain, and mutation pattern. There is no single
   best runtime representation for `List<T>` or `Map<K,V>`.
8. Keep macro arguments typed. Function IDs, NBT paths, keys, and SNBT patterns are
   command syntax, not ordinary untrusted strings.

## What MDL should improve

The surveyed libraries prove a large API is possible, but many implementations
copy an input and repeatedly remove element zero. Because Java 26.2 stores an NBT
list in an array-backed `ListTag`, that traversal shifts the remainder every time
and is structurally quadratic. A compiler can retain the same API while choosing
tail traversal, dynamic indexing, reversal, a zipper, or a specialized structure.

Likewise, shared global I/O storage plus manually pushed fields works, but typed
compiler-generated frames can make aliasing, ownership, re-entrancy, callback
effects, and cleanup explicit.
