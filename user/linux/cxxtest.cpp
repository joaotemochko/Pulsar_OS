// cxxtest — a biblioteca C++ dos programas Linux do Pulsar (libc++, libc++abi
// e libunwind do LLVM): excecoes atravessando quadros (com destrutores RAII
// no caminho), RTTI, conteineres, iostream, std::thread + mutex + variavel de
// condicao, chrono e construtores globais. Termina com "fim: N falha(s)".
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <typeinfo>
#include <vector>

static int falhas;
static void check(bool ok, const char* what) {
    std::printf("[cxxtest] %s    %s\n", ok ? "ok  " : "FALHOU", what);
    if (!ok) falhas++;
}

// construtor global: roda antes do main (init_array)
struct Global { int v = 0; Global() { v = 42; } };
static Global g_global;

// ---- excecoes ----
static int destroyed;
struct Guard { ~Guard() { destroyed++; } };

struct Erro : std::runtime_error {
    int code;
    Erro(const char* m, int c) : std::runtime_error(m), code(c) {}
};

static int fundo(int n) {
    Guard g;
    if (n == 0) throw Erro("do fundo", 7);
    return fundo(n - 1) + 1;
}

static bool excecoes() {
    destroyed = 0;
    try {
        fundo(5);
    } catch (const std::exception& e) {      // pega pela base
        const Erro* er = dynamic_cast<const Erro*>(&e);
        return er && er->code == 7 && std::string(e.what()) == "do fundo" && destroyed == 6;
    }
    return false;
}

static bool rethrow() {
    try {
        try { throw std::out_of_range("x"); }
        catch (...) { throw; }
    } catch (const std::logic_error&) { return true; }
    return false;
}

// ---- RTTI ----
struct Base { virtual ~Base() = default; virtual int f() const { return 1; } };
struct Derivada : Base { int f() const override { return 2; } };

int main() {
    check(g_global.v == 42, "construtor global antes do main");

    check(excecoes(), "excecao atravessa 6 quadros, destrutores RAII rodam, catch pela base");
    check(rethrow(), "throw; repassa a excecao para o catch de fora");
    try { std::vector<int> v(3); (void)v.at(10); check(false, "vector::at fora do limite lanca"); }
    catch (const std::out_of_range&) { check(true, "vector::at fora do limite lanca std::out_of_range"); }

    std::unique_ptr<Base> b = std::make_unique<Derivada>();
    check(dynamic_cast<Derivada*>(b.get()) != nullptr && b->f() == 2, "dynamic_cast e chamada virtual");
    const Base& rb = *b;
    check(typeid(rb) == typeid(Derivada), "typeid de objeto polimorfico");

    std::map<std::string, int> m{{"tres", 3}, {"um", 1}, {"dois", 2}};
    std::vector<std::string> keys;
    for (auto& [k, v] : m) keys.push_back(k + "=" + std::to_string(v));
    std::sort(keys.begin(), keys.end(), [](auto& a, auto& b) { return a.size() < b.size() || (a.size() == b.size() && a < b); });
    check(keys.front() == "um=1" && keys.back() == "tres=3", "map, string, to_string, sort com lambda");

    std::ostringstream os;
    os << "pi ~ " << 3.14159 << ", hex " << std::hex << 255;
    check(os.str() == "pi ~ 3.14159, hex ff", "ostringstream (locale \"C\", double, hex)");
    std::cout << "[cxxtest] ola do std::cout: " << os.str() << std::endl;

    // threads: 4 threads somam num atomico e num contador sob mutex; a
    // principal espera um aviso por variavel de condicao
    std::atomic<int> soma{0};
    int protegido = 0;
    std::mutex mu;
    std::condition_variable cv;
    int prontas = 0;
    auto t0 = std::chrono::steady_clock::now();
    std::vector<std::thread> ts;
    for (int i = 0; i < 4; i++)
        ts.emplace_back([&, i] {
            for (int k = 0; k < 1000; k++) {
                soma.fetch_add(1, std::memory_order_relaxed);
                std::lock_guard<std::mutex> l(mu);
                protegido++;
            }
            { std::lock_guard<std::mutex> l(mu); prontas++; }
            cv.notify_one();
            (void)i;
        });
    {
        std::unique_lock<std::mutex> l(mu);
        cv.wait(l, [&] { return prontas == 4; });
    }
    for (auto& t : ts) t.join();
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - t0).count();
    check(soma == 4000 && protegido == 4000, "4 std::thread: atomico + mutex somam 4000");
    check(prontas == 4, "condition_variable acorda a thread principal");
    std::printf("[cxxtest] threads em %lld ms (hardware_concurrency = %u)\n", (long long)ms, std::thread::hardware_concurrency());

    // excecao lancada dentro de uma thread e capturada nela
    bool pegou = false;
    std::thread([&] { try { throw 5; } catch (int v) { pegou = v == 5; } }).join();
    check(pegou, "excecao dentro de uma std::thread");

    std::printf("[cxxtest] fim: %d falha(s)\n", falhas);
    return falhas ? 1 : 0;
}
