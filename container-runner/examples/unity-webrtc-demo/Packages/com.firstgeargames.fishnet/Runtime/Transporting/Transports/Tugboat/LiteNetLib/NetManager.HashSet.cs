using System;
using System.Net;
using System.Threading;

namespace LiteNetLib
{
    // minimal hashset class from dotnet with some optimizations
    public partial class NetManager
    {
        private const int MaxPrimeArrayLength = 0x7FFFFFC3;
        private const int HashPrime = 101;
        private const int Lower31BitMask = 0x7FFFFFFF;
        private static readonly int[] Primes =
        {
            3, 7, 11, 17, 23, 29, 37, 47, 59, 71, 89, 107, 131, 163, 197, 239, 293, 353, 431, 521, 631, 761, 919,
            1103, 1327, 1597, 1931, 2333, 2801, 3371, 4049, 4861, 5839, 7013, 8419, 10103, 12143, 14591,
            17519, 21023, 25229, 30293, 36353, 43627, 52361, 62851, 75431, 90523, 108631, 130363, 156437,
            187751, 225307, 270371, 324449, 389357, 467237, 560689, 672827, 807403, 968897, 1162687, 1395263,
            1674319, 2009191, 2411033, 2893249, 3471899, 4166287, 4999559, 5999471, 7199369
        };

        private static int HashSetGetPrime(int min)
        {
            foreach (int prime in Primes)
            {
                if (prime >= min)
                    return prime;
            }

            // Outside of our predefined table. Compute the hard way.
            for (int i = min | 1; i < int.MaxValue; i += 2)
            {
                if (IsPrime(i) && (i - 1) % HashPrime != 0)
                    return i;
            }
            return min;

            bool IsPrime(int candidate)
            {
                if ((candidate & 1) != 0)
                {
                    int limit = (int)Math.Sqrt(candidate);
                    for (int divisor = 3; divisor <= limit; divisor += 2)
                    {
                        if (candidate % divisor == 0)
                            return false;
                    }
                    return true;
                }
                return candidate == 2;
            }
        }

        private struct Slot
        {
            internal int HashCode;
            internal int Next;
            internal NetPeer Value;
        }

        private int[] _buckets;
        private Slot[] _slots;
        private int _count;
        private int _lastIndex;
        private int _freeList = -1;
        private NetPeer[] _peersArray = new NetPeer[32];
        private readonly ReaderWriterLockSlim _peersLock = new(LockRecursionPolicy.NoRecursion);
        private volatile NetPeer _headPeer;

        private void ClearPeerSet()
        {
            _peersLock.EnterWriteLock();
            _headPeer = null;
            if (_lastIndex > 0)
            {
                Array.Clear(_slots, 0, _lastIndex);
                Array.Clear(_buckets, 0, _buckets.Length);
                _lastIndex = 0;
                _count = 0;
                _freeList = -1;
            }
            _peersArray = new NetPeer[32];
            _peersLock.ExitWriteLock();
        }

        private bool ContainsPeer(NetPeer item)
        {
            if (_buckets != null)
            {
                int hashCode = item.GetHashCode() & Lower31BitMask;
                for (int i = _buckets[hashCode % _buckets.Length] - 1; i >= 0; i = _slots[i].Next)
                {
                    if (_slots[i].HashCode == hashCode && _slots[i].Value.Equals(item))
                        return true;
                }
            }
            return false;
        }

        /// <summary>
        /// Gets peer by peer id
        /// </summary>
        /// <param name = "id">id of peer</param>
        /// <returns>Peer if peer with id exist, otherwise null</returns>
        public NetPeer GetPeerById(int id)
        {
            return id >= 0 && id < _peersArray.Length ? _peersArray[id] : null;
        }

        /// <summary>
        /// Gets peer by peer id
        /// </summary>
        /// <param name = "id">id of peer</param>
        /// <param name = "peer">resulting peer</param>
        /// <returns>True if peer with id exist, otherwise false</returns>
        public bool TryGetPeerById(int id, out NetPeer peer)
        {
            peer = GetPeerById(id);
            return peer != null;
        }

        private void AddPeer(NetPeer peer)
        {
            _peersLock.EnterWriteLock();
            if (_headPeer != null)
            {
                peer.NextPeer = _headPeer;
                _headPeer.PrevPeer = peer;
            }
            _headPeer = peer;
            AddPeerToSet(peer);
            if (peer.Id >= _peersArray.Length)
            {
                int newSize = _peersArray.Length * 2;
                while (peer.Id >= newSize)
                    newSize *= 2;
                Array.Resize(ref _peersArray, newSize);
            }
            _peersArray[peer.Id] = peer;
            _peersLock.ExitWriteLock();
        }

        private void RemovePeer(NetPeer peer)
        {
            _peersLock.EnterWriteLock();
            RemovePeerInternal(peer);
            _peersLock.ExitWriteLock();
        }

        private void RemovePeerInternal(NetPeer peer)
        {
            if (!RemovePeerFromSet(peer))
                return;
            if (peer == _headPeer)
                _headPeer = peer.NextPeer;

            if (peer.PrevPeer != null)
                peer.PrevPeer.NextPeer = peer.NextPeer;
            if (peer.NextPeer != null)
                peer.NextPeer.PrevPeer = peer.PrevPeer;
            peer.PrevPeer = null;

            _peersArray[peer.Id] = null;
            _peerIds.Enqueue(peer.Id);
        }

        private bool RemovePeerFromSet(NetPeer peer)
        {
            if (_buckets == null)
                return false;
            if (peer == null)
                return false;
            int hashCode = peer.GetHashCode() & Lower31BitMask;
            int bucket = hashCode % _buckets.Length;
            int last = -1;
            for (int i = _buckets[bucket] - 1; i >= 0; last = i, i = _slots[i].Next)
            {
                if (_slots[i].HashCode == hashCode && _slots[i].Value.Equals(peer))
                {
                    if (last < 0)
                        _buckets[bucket] = _slots[i].Next + 1;
                    else
                        _slots[last].Next = _slots[i].Next;
                    _slots[i].HashCode = -1;
                    _slots[i].Value = null;
                    _slots[i].Next = _freeList;

                    _count--;
                    if (_count == 0)
                    {
                        _lastIndex = 0;
                        _freeList = -1;
                    }
                    else
                    {
                        _freeList = i;
                    }
                    return true;
                }
            }
            return false;
        }

        private bool TryGetPeer(IPEndPoint endPoint, out NetPeer actualValue)
        {
            if (_buckets != null)
            {
#if NET8_0_OR_GREATER
                // can be NetPeer or IPEndPoint
                int hashCode = (UseNativeSockets ? endPoint.GetHashCode() : endPoint.Serialize().GetHashCode()) & Lower31BitMask;
#else
                int hashCode = endPoint.GetHashCode() & Lower31BitMask;
#endif
                _peersLock.EnterReadLock();
                for (int i = _buckets[hashCode % _buckets.Length] - 1; i >= 0; i = _slots[i].Next)
                {
                    if (_slots[i].HashCode == hashCode && _slots[i].Value.Equals(endPoint))
                    {
                        actualValue = _slots[i].Value;
                        _peersLock.ExitReadLock();
                        return true;
                    }
                }
                _peersLock.ExitReadLock();
            }
            actualValue = null;
            return false;
        }

        // only used for NET8
        private bool TryGetPeer(SocketAddress saddr, out NetPeer actualValue)
        {
            if (_buckets != null)
            {
                int hashCode = saddr.GetHashCode() & Lower31BitMask;
                _peersLock.EnterReadLock();
                for (int i = _buckets[hashCode % _buckets.Length] - 1; i >= 0; i = _slots[i].Next)
                {
                    if (_slots[i].HashCode == hashCode && _slots[i].Value.Serialize().Equals(saddr))
                    {
                        actualValue = _slots[i].Value;
                        _peersLock.ExitReadLock();
                        return true;
                    }
                }
                _peersLock.ExitReadLock();
            }
            actualValue = null;
            return false;
        }

        private bool AddPeerToSet(NetPeer value)
        {
            if (_buckets == null)
            {
                int size = HashSetGetPrime(0);
                _buckets = new int[size];
                _slots = new Slot[size];
            }

            int hashCode = value.GetHashCode() & Lower31BitMask;
            int bucket = hashCode % _buckets.Length;
            for (int i = _buckets[hashCode % _buckets.Length] - 1; i >= 0; i = _slots[i].Next)
            {
                if (_slots[i].HashCode == hashCode && _slots[i].Value.Equals(value))
                    return false;
            }

            int index;
            if (_freeList >= 0)
            {
                index = _freeList;
                _freeList = _slots[index].Next;
            }
            else
            {
                if (_lastIndex == _slots.Length)
                {
                    // increase capacity
                    int newSize = 2 * _count;
                    newSize = (uint)newSize > MaxPrimeArrayLength && MaxPrimeArrayLength > _count ? MaxPrimeArrayLength : HashSetGetPrime(newSize);

                    // Able to increase capacity; copy elements to larger array and rehash
                    Slot[] newSlots = new Slot[newSize];
                    Array.Copy(_slots, 0, newSlots, 0, _lastIndex);
                    _buckets = new int[newSize];
                    for (int i = 0; i < _lastIndex; i++)
                    {
                        int b = newSlots[i].HashCode % newSize;
                        newSlots[i].Next = _buckets[b] - 1;
                        _buckets[b] = i + 1;
                    }
                    _slots = newSlots;
                    // this will change during resize
                    bucket = hashCode % _buckets.Length;
                }
                index = _lastIndex;
                _lastIndex++;
            }
            _slots[index].HashCode = hashCode;
            _slots[index].Value = value;
            _slots[index].Next = _buckets[bucket] - 1;
            _buckets[bucket] = index + 1;
            _count++;

            return true;
        }
    }
}