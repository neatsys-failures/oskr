#pragma once
#include <experimental/coroutine>
// it is surprised that core model not make use of optional anywhere :)
#include <optional>
#include <variant>

#include <bitsery/brief_syntax/variant.h>

#include "core/Bitsery.hpp"
#include "core/Client.hpp"
#include "core/Log.hpp"
#include "core/Transport.hpp"
#include "core/TransportReceiver.hpp"
#include "core/Type.hpp"
#include "core/Utility.hpp"
