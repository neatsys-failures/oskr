// final decision: will not use co_await

#include <deque>
#include <experimental/coroutine>
#include <iostream>
#include <thread>
#include <vector>

using namespace std;
using namespace std::experimental;

struct Guest;
struct Runtime {
    void run(Guest guest, int arg);

    struct Task;
    struct TaskPromise {
        int code;
        TaskPromise(Guest &guest, int arg);
        TaskPromise() {}

        suspend_always initial_suspend() { return {}; }
        suspend_never final_suspend() noexcept { return {}; }
        void unhandled_exception() {}
        Task get_return_object() { return {*this}; }
        void return_void() {}
    };
    struct Task {
        TaskPromise &promise;
        using promise_type = TaskPromise;
    };

    struct SwitchAwaitable {
        Runtime &runtime;
        coroutine_handle<TaskPromise> handle{};
        int resume_id{};

        bool await_ready() { return false; }
        int await_resume() { return resume_id; }
        void await_suspend(coroutine_handle<TaskPromise> handle)
        {
            this->handle = handle;
            runtime.switch_list.push_back(this);
        }
    };
    deque<SwitchAwaitable *> switch_list;

    SwitchAwaitable switchResume() { return {*this}; }
};

struct NestedAwaitable {
    bool await_ready() { return false; }
    void await_resume() {}
    void await_suspend(coroutine_handle<> handle)
    {
        coroutine_handle<Runtime::TaskPromise>::from_promise(task.promise)
            .resume();
    }

    Runtime::Task task;
    NestedAwaitable(Runtime::Task nested_task) : task(nested_task) {}
};

struct Guest {
    Runtime &runtime;
    Guest(Runtime &runtime) : runtime(runtime) {}
    int getCode(int arg) { return arg * 2; }

    Runtime::Task start(int arg)
    {
        cout << "enter task scope, thread id = " << this_thread::get_id()
             << "\n";
        auto tid = co_await runtime.switchResume();
        cout << "switch: tid = " << tid
             << ", thread id = " << this_thread::get_id() << "\n";
        tid = co_await runtime.switchResume();
        cout << "switch again: tid = " << tid
             << ", thread id = " << this_thread::get_id() << "\n";
    }

    Runtime::Task nestedCall()
    {
        cout << "nested co_await\n";
        co_await runtime.switchResume();
        cout << "nested co_await done\n";
    }
};

Runtime::TaskPromise::TaskPromise(Guest &guest, int arg)
{
    code = guest.getCode(arg);
}

void Runtime::run(Guest guest, int arg)
{
    auto task = guest.start(arg);
    cout << "Task initialized: code = " << task.promise.code << "\n";
    auto awaitable = new SwitchAwaitable{
        *this, coroutine_handle<TaskPromise>::from_promise(task.promise)};
    switch_list.push_back(awaitable);

    int resume_id = 0;
    while (!switch_list.empty()) {
        auto &awaitable = *switch_list.front();
        switch_list.pop_front();
        resume_id += 1;
        awaitable.resume_id = resume_id;
        auto used = thread([&]() mutable {
            cout << "spawn worker thread " << this_thread::get_id() << "\n";
            awaitable.handle.resume();
        });
        used.join();
    }
}

int main()
{
    Runtime runtime;
    runtime.run(Guest(runtime), 67);
    return 0;
}
