#pragma once
#include <functional>

#include "core/Type.hpp"
#include "core/Utility.hpp"

namespace oskr
{
class App
{
public:
    virtual ~App() {}
    virtual Data commit(Data op) = 0;
    virtual void rollback(Data) { panic("Unsupported rollback operation"); }
};

/*! @brief Log is the representation of state machine (replication or other).

Nothing to do with logging stuff.

The templated class `Log` try to extract similar thing from different kinds of
log: `Log<>::List`, `Log<>::Chain`, maybe more in the future. These kinds of
logs are still abstract interfaces, there are corresponding conventional
implementation for them in `common`, named `ListLog` and `ChainLog`.

(That said the structure is not very different than directly define abstract
`LogList` and `LogChain` interfaces. I just hate to repeat similar code :)

One of the major differences between Oskr and specpaxos is the choice on log
abstraction/component. While specpaxos exclude log from protocol abstraction
(instead providing a reusable log component) and require protocol to manipulate
application directly, Oskr hide application behind log abstraction. The design
decision is made because:
- A state machine application should always equivalent to its initial state +
an log, i.e., all the transitions applied to the state machine. The two things
should always be kept in synchronized so it is not necessary to expose both to
protocols.
- Require protocols to *reference* to log instead of to *own* the log giving
external access to the log. This may enable some useful cases:
  - (Imaginary) log persistent, which probably reasonablly be transparent to
  protocol implementation (in the sense of technical details, protocol still
  should take charge of controlling the stable committed point).
  - Fine-grained test cases. In specpaxos user can only exam protocol's progress
  after op got committed. With an accessible log she is able to test more.

The abstraction-style log results in a reduced concept. The `Log` abstraction
does not contain any concrete date structure, only pure interfaces. The
abstraction also eliminate all protocol-specific log states except a
prepare-commit pair, which is a too common pattern.

Last but not least, this `Log` (or more precisely, `Log<>::List` and
`Log<>::Chain`) has a batching concept built in. The manipulating unit is a
group of op instead of a single one (terminology *block* is chosen because of
blockchain, but some protocols may prefer using *batch* to refer to it), and
all op in the same block share an log index. This feature should simplify
certain protocol's implementation. If necessary, a custom non-batching `Preset`
can be used as type argument to create a non-batching log abstraction.
*/
template <typename Preset = void> class Log
{
protected:
    App &app;
    bool enable_upcall;

    //! Each log instance should keep a reference to an `App`. The reference
    //! semantic is chosen instead of value semantic because most applications
    //! do not care what kind of `Log` is controlling them. Exposing app to make
    //! it observable to outer world may also benefit in some situation.
    Log(App &app) : app(app) { enable_upcall = true; }

public:
    using Index = typename Preset::Index;
    using Block = typename Preset::Block;

    virtual ~Log() {}

    //! Preparing a block means to insert `block` content into local memory.
    //! Probably the `block` will be committed, but it is fine to replace the
    //! block with another later `prepare` call on the same `index`. (Although
    //! log implementation will probably want to warn.) However, calling
    //! `prepare` after `commit` on the `index` voilates the invariant and is
    //! always a fatal error.
    //!
    //! The `index` that is valid to `prepare` is usually limited. Basically log
    //! does not allow "gap" between prepared blocks, so protocol should
    //! construct reordering buffer by itself if necessary.
    //!
    //! @note The timing for calling `prepare` is not necessary to match the
    //! *prepare* stage of certain protocols. For example for PBFT `prepare`
    //! should be called in the pre-prepare stage. The method names `prepare`
    //! and `commit` are simply adapted from popular viewstamped replication and
    //! two-phase commit protocols. `prepare` just correspond to "data is ready"
    //! and `commit` just correspond to "data is stable", but not to any
    //! protocol-specific details. For tracing any extra state related to
    //! certain block (e.g., whether pre-prepared or prepared in PBFT), protocol
    //! should construct parallel data structure separately. This is also the
    //! reason of intentionally-lacked getter interface of `Log`.
    virtual void prepare(Index index, Block block) = 0;

    //! A callback that consume information of the execution of a op per
    //! invoking. Mainly used to reply to client.
    using ReplyCallback = Fn<void(ClientId, RequestNumber, Data)>;
    //! Commit the block at `index`, it is a fatal error if the `index` has not
    //! been `prepared` before.
    //!
    //! The benefit of including prepare-commit concept into abstraction is to
    //! enable out-of-order committing. A log implementation promise to execute
    //! a block with application as soon as the block along with all blocks
    //! prior to it have committed. During the executing `callback` will be
    //! invoked once per every executed op. The callback may be invoked multiple
    //! times in one `commit` call because there are multiple op in the
    //! committed block, and/or because multiple blocks are executed by this
    //! committing.
    //!
    //! @note Although `commit` should correspond to *finalize*, in this log
    //! model, a call to `commit` actually can be reverted by a `rollbackTo`
    //! calling. This means currently there is no way to assert a block will be
    //! permanent forever. So the precise definition of `commit` method is to
    //! speculative execute blocks. Specpaxos provide another method to do the
    //! actually "commit" on speculated blocks, maybe there will be one here as
    //! well.
    virtual void commit(Index index, ReplyCallback callback) = 0;
    // commitUpTo should be unusual for fast path

    //! Rollback everything after `index` including itself. If there is a
    //! executed part included in the rollback blocks, the block in that part
    //! will be rollbacked on application in reversed order. To rollback a
    //! block, every op in the block will be passed to application's `rollback`
    //! call in reversed order.
    virtual void rollbackTo(Index index) = 0;

    //! This will not only set `enable_upcall` flag, but also execute all blocks
    //! that are valid to be executed at this point.
    //!
    //! The execution is silenced because a reply to client at this point is
    //! meaningless.
    virtual void enableUpcall() = 0;

    //! @note Disable upcall is not the same as "quiet backup". For example in
    //! viewstamped replication, all backups should execute op (i.e., enable
    //! upcall), just not reply to client. In this case the correct way is to
    //! provide a no-op `callback` to `commit`.
    virtual void disableUpcall() { enable_upcall = false; }
};

/*! Common related things for all kinds of logs.

Define in a specialization to prevent cyclic initialization.
*/
template <> struct Log<void> {
    struct Entry {
        ClientId client_id;
        RequestNumber request_number;
        Data op;
    };
    static constexpr int block_size = 50; // TODO
    // expect 600K~1M throughput @ <= 60 seconds
    static constexpr std::size_t n_reserved_entry = 80 * 1000 * 1000;

    struct ListPreset {
        using Index = OpNumber;
        struct Block {
            Entry entry_buffer[block_size];
            int n_entry;
        };
    };
    //! @brief The abstraction of a list-like log structure.
    //!
    //! Every block is indexed with a `OpNumber`, normally should start from 1.
    //! (`ListLog` supports starting from the middle in the case of recovery.)
    //! New prepared block is appended to the end.
    //!
    //! The linear abstraction means there will always be only one candidate for
    //! every in-order block, which is the case of non-BFT setup.
    using List = Log<ListPreset>;

    struct ChainPreset {
        using Index = Hash;
        struct Block {
            Entry entry_buffer[block_size];
            int n_entry;
            Hash previous;
        };
    };
    //! @brief The abstraction of a tree-like log structure, i.e., blockchain.
    //!
    //! There can be multiple blocks with the same "depth", i.e., has the same
    //! `previous` content. Only at most one of these blocks can be committed
    //! (i.e., all the others are treated as malicious). This is designed for a
    //! BFT environment, where multiple branches may exist, but only one of them
    //! can reach consensus eventually.
    using Chain = Log<ChainPreset>;
};

} // namespace oskr