#include <experimental/coroutine>
#include <iostream>
#include <thread>
#include <vector>

using namespace std;
using namespace std::experimental;

struct Guest;
struct Runtime {
    Runtime() { resume_id = 0; }
    void run(Guest guest, int arg);

    template <typename Guest> struct Task;
    template <typename Guest> struct TaskPromise {
        int code;
        TaskPromise(Guest &guest, int arg) { code = guest.getCode(arg); }

        suspend_never initial_suspend() { return {}; }
        suspend_never final_suspend() noexcept { return {}; }
        void unhandled_exception() {}
        Task<Guest> get_return_object() { return {*this}; }
        void return_void() {}
    };
    template <typename Guest> struct Task {
        TaskPromise<Guest> &promise;
        using promise_type = TaskPromise<Guest>;
    };

    struct SwitchAwaitable;
    vector<SwitchAwaitable> switch_list;
    int resume_id;
    struct SwitchAwaitable {
        Runtime &runtime;
        coroutine_handle<> handle{};
        int resume_id{};

        bool await_ready() { return false; }
        int await_resume() { return resume_id; }
        void await_suspend(coroutine_handle<> handle)
        {
            this->handle = handle;
            runtime.switch_list.push_back(*this);
        }
    };
    SwitchAwaitable switchResume() { return {*this}; }
};

struct Guest {
    Runtime &runtime;
    Guest(Runtime &runtime) : runtime(runtime) {}
    int getCode(int arg) { return arg * 2; }

    Runtime::Task<Guest> start(int arg)
    {
        cout << "enter task scope, thread id = " << this_thread::get_id()
             << "\n";
        auto tid = co_await runtime.switchResume();
        cout << "switch: tid = " << tid
             << ", thread id = " << this_thread::get_id();
        tid = co_await runtime.switchResume();
        cout << "switch again: tid = " << tid
             << ", thread id = " << this_thread::get_id();
    }
};

void Runtime::run(Guest guest, int arg)
{
    //
}

int main()
{
    Runtime runtime;
    runtime.run(Guest(runtime), 67);
    return 0;
}
