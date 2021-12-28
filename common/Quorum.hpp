// Adapted:
// https://github.com/UWSysLab/specpaxos/blob/master/common/quorumset.h

// -*- mode: c++; c-file-style: "k&r"; c-basic-offset: 4 -*-
/***********************************************************************
 *
 * quorumset.h:
 *   utility type for tracking sets of messages received from other
 *   replicas and determining whether a quorum of responses has been met
 *
 * Copyright 2013-2016 Dan R. K. Ports  <drkp@cs.washington.edu>
 *
 * Permission is hereby granted, free of charge, to any person
 * obtaining a copy of this software and associated documentation
 * files (the "Software"), to deal in the Software without
 * restriction, including without limitation the rights to use, copy,
 * modify, merge, publish, distribute, sublicense, and/or sell copies
 * of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 **********************************************************************/

#ifndef _COMMON_QUORUMSET_H_
#define _COMMON_QUORUMSET_H_

#include <map>
#include <unordered_map>

#include "core/Foundation.hpp"

namespace oskr
{

template <class IDTYPE, class MSGTYPE> class Quorum
{
public:
    Quorum(int n_required) : numRequired(n_required) {}

    void clear() { messages.clear(); }

    void clear(IDTYPE vs)
    {
        std::map<ReplicaId, MSGTYPE> &vsmessages = messages[vs];
        vsmessages.clear();
    }

    ReplicaId nRequired() const { return numRequired; }

    const std::map<ReplicaId, MSGTYPE> &getMessages(IDTYPE vs)
    {
        return messages[vs];
    }

    // there is no optional reference in C++ by now
    const std::map<ReplicaId, MSGTYPE> *checkForQuorum(IDTYPE vs)
    {
        std::map<ReplicaId, MSGTYPE> &vsmessages = messages[vs];
        int count = vsmessages.size();
        if (count >= numRequired) {
            return &vsmessages;
        } else {
            return nullptr;
        }
    }

    const std::map<ReplicaId, MSGTYPE> *
    addAndCheckForQuorum(IDTYPE vs, ReplicaId replica_id, const MSGTYPE &msg)
    {
        std::map<ReplicaId, MSGTYPE> &vsmessages = messages[vs];
        if (vsmessages.find(replica_id) != vsmessages.end()) {
            // This is a duplicate message

            // But we'll ignore that, replace the old message from
            // this replica, and proceed.
            //
            // XXX Is this the right thing to do? It is for
            // speculative replies in SpecPaxos...
        }

        vsmessages[replica_id] = msg;

        return checkForQuorum(vs);
    }

    void add(IDTYPE vs, ReplicaId replica_id, const MSGTYPE &msg)
    {
        addAndCheckForQuorum(vs, replica_id, msg);
    }

public:
    int numRequired;

private:
    std::unordered_map<IDTYPE, std::map<ReplicaId, MSGTYPE>> messages;
};

} // namespace oskr

#endif // _COMMON_QUORUMSET_H_