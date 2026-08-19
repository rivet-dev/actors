using System;
using System.Collections.Concurrent;
using GameKit.Dependencies.Utilities.Types;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

// ReSharper disable ThreadStaticFieldHasInitializesr
namespace GameKit.Dependencies.Utilities
{
    /// <summary>
    /// Implement to use type with Caches.
    /// </summary>
    public interface IResettable
    {
        /// <summary>
        /// Resets values when being placed in a cache.
        /// </summary>
        void ResetState();

        /// <summary>
        /// Initializes values after being retrieved from a cache.
        /// </summary>
        void InitializeState();
    }

    #region Resettable caches.
    /// <summary>
    /// Caches collections of multiple generics.
    /// </summary>
    public static class ResettableCollectionCaches<T1, T2> where T1 : IResettable, new() where T2 : IResettable, new()
    {
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ResettableCollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Dictionary<T1, T2> RetrieveDictionary() => CollectionCaches<T1, T2>.RetrieveDictionary();

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (KeyValuePair<T1, T2> kvp in value)
                {
                    ResettableObjectCaches<T1>.Store(kvp.Key);
                    ResettableObjectCaches<T2>.Store(kvp.Value);
                }

                value.Clear();

                CollectionCaches<T1, T2>.Store(value);
            }
        }
    }

    /// <summary>
    /// Caches collections of multiple generics.
    /// </summary>
    public static class ResettableT1CollectionCaches<T1, T2> where T1 : IResettable, new()
    {
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ResettableT1CollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Dictionary<T1, T2> RetrieveDictionary() => CollectionCaches<T1, T2>.RetrieveDictionary();

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (T1 item in value.Keys)
                    ResettableObjectCaches<T1>.Store(item);

                value.Clear();
                CollectionCaches<T1, T2>.Store(value);
            }
        }
    }

    /// <summary>
    /// Caches collections of multiple generics.
    /// </summary>
    public static class ResettableT2CollectionCaches<T1, T2> where T2 : IResettable, new()
    {
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ResettableT2CollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Dictionary<T1, T2> RetrieveDictionary() => CollectionCaches<T1, T2>.RetrieveDictionary();

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (T2 item in value.Values)
                    ResettableObjectCaches<T2>.Store(item);

                value.Clear();
                CollectionCaches<T1, T2>.Store(value);
            }
        }
    }

    /// <summary>
    /// Caches collections of a single generic.
    /// </summary>
    public static class ResettableCollectionCaches<T> where T : IResettable, new()
    {
        /// <summary>
        /// Cache for ResettableRingBuffer.
        /// </summary>
        private static readonly Stack<ResettableRingBuffer<T>> _resettableRingBufferCache = new();
        /// <summary>
        /// Maximum number of entries allowed for the cache.
        /// </summary>
        private const int MAXIMUM_CACHE_COUNT = 50;
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ResettableCollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        public static ResettableRingBuffer<T> RetrieveRingBuffer()
        {
            lock (_lock)
            {
                ResettableRingBuffer<T> result;
                if (!_resettableRingBufferCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static T[] RetrieveArray() => CollectionCaches<T>.RetrieveArray();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static List<T> RetrieveList() => CollectionCaches<T>.RetrieveList();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static SortedSet<T> RetrieveSortedSet() => CollectionCaches<T>.RetrieveSortedSet();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static HashSet<T> RetrieveHashSet() => CollectionCaches<T>.RetrieveHashSet();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Queue<T> RetrieveQueue() => CollectionCaches<T>.RetrieveQueue();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static BasicQueue<T> RetrieveBasicQueue() => CollectionCaches<T>.RetrieveBasicQueue();

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array from the beginning.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref ResettableRingBuffer<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array from the beginning.</param>
        public static void Store(ResettableRingBuffer<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.ResetState();

                if (_resettableRingBufferCache.Count < MAXIMUM_CACHE_COUNT)
                    _resettableRingBufferCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array from the beginning.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref T[] value, int count)
        {
            lock (_lock)
            {
                Store(value, count);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array from the beginning.</param>
        public static void Store(T[] value, int count)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                for (int i = 0; i < count; i++)
                    ResettableObjectCaches<T>.Store(value[i]);

                CollectionCaches<T>.Store(value, count);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref List<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(List<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                for (int i = 0; i < value.Count; i++)
                    ResettableObjectCaches<T>.Store(value[i]);

                value.Clear();
                CollectionCaches<T>.Store(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref SortedSet<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(SortedSet<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (T item in value)
                    ResettableObjectCaches<T>.Store(item);

                value.Clear();
                CollectionCaches<T>.Store(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref HashSet<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(HashSet<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (T item in value)
                    ResettableObjectCaches<T>.Store(item);

                value.Clear();
                CollectionCaches<T>.Store(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Queue<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Queue<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                foreach (T item in value)
                    ResettableObjectCaches<T>.Store(item);

                value.Clear();
                CollectionCaches<T>.Store(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref BasicQueue<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(BasicQueue<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                while (value.TryDequeue(out T result))
                    ResettableObjectCaches<T>.Store(result);

                value.Clear();
                CollectionCaches<T>.Store(value);
            }
        }
    }

    /// <summary>
    /// Caches objects of a single generic.
    /// </summary>
    public static class ResettableObjectCaches<T> where T : IResettable, new()
    {
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ResettableObjectCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves an instance of T.
        /// </summary>
        public static T Retrieve()
        {
            lock (_lock)
            {
                T result = ObjectCaches<T>.Retrieve();
                result.InitializeState();
                return result;
            }
        }

        /// <summary>
        /// Stores an instance of T and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref T value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores an instance of T.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(T value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.ResetState();
                ObjectCaches<T>.Store(value);
            }
        }
    }
    #endregion

    #region NonResettable caches.
    /// <summary>
    /// Caches collections of multiple generics.
    /// </summary>
    public static class CollectionCaches<T1, T2>
    {
        /// <summary>
        /// Cache for dictionaries.
        /// </summary>
        private static readonly Stack<Dictionary<T1, T2>> _dictionaryCache = new();
        /// <summary>
        /// Maximum number of entries allowed for the cache.
        /// </summary>
        private const int MAXIMUM_CACHE_COUNT = 50;
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static CollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Dictionary<T1, T2> RetrieveDictionary()
        {
            lock (_lock)
            {
                Dictionary<T1, T2> result;
                if (!_dictionaryCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Dictionary<T1, T2> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();
                if (_dictionaryCache.Count < MAXIMUM_CACHE_COUNT)
                    _dictionaryCache.Push(value);
            }
        }
    }

    /// <summary>
    /// Caches collections of a single generic.
    /// </summary>
    public static partial class CollectionCaches<T>
    {
        /// <summary>
        /// Cache for arrays.
        /// </summary>
        private static readonly Stack<T[]> _arrayCache = new();
        /// <summary>
        /// Cache for lists.
        /// </summary>
        private static readonly Stack<List<T>> _listCache = new();
        /// <summary>
        /// Cache for sortedset.
        /// </summary>
        private static readonly Stack<SortedSet<T>> _sortedSetCache = new();
        /// <summary>
        /// Cache for queues.
        /// </summary>
        private static readonly Stack<Queue<T>> _queueCache = new();
        /// <summary>
        /// Cache for queues.
        /// </summary>
        private static readonly Stack<BasicQueue<T>> _basicQueueCache = new();
        /// <summary>
        /// Cache for hashset.
        /// </summary>
        private static readonly Stack<HashSet<T>> _hashSetCache = new();
        /// <summary>
        /// Maximum number of entries allowed for the cache.
        /// </summary>
        private const int MAXIMUM_CACHE_COUNT = 50;
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static CollectionCaches()
        {
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static T[] RetrieveArray()
        {
            lock (_lock)
            {
                T[] result;
                if (!_arrayCache.TryPop(out result))
                    result = new T[0];

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static List<T> RetrieveList()
        {
            lock (_lock)
            {
                List<T> result;
                if (!_listCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static SortedSet<T> RetrieveSortedSet()
        {
            lock (_lock)
            {
                SortedSet<T> result;
                if (!_sortedSetCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static Queue<T> RetrieveQueue()
        {
            lock (_lock)
            {
                Queue<T> result;
                if (!_queueCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection.
        /// </summary>
        /// <returns></returns>
        public static BasicQueue<T> RetrieveBasicQueue()
        {
            lock (_lock)
            {
                BasicQueue<T> result;
                if (!_basicQueueCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection adding one entry.
        /// </summary>
        /// <returns></returns>
        public static Queue<T> RetrieveQueue(T entry)
        {
            lock (_lock)
            {
                Queue<T> result;
                if (!_queueCache.TryPop(out result))
                    result = new();

                result.Enqueue(entry);
                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection adding one entry.
        /// </summary>
        /// <returns></returns>
        public static List<T> RetrieveList(T entry)
        {
            lock (_lock)
            {
                List<T> result;
                if (!_listCache.TryPop(out result))
                    result = new();

                result.Add(entry);
                return result;
            }
        }

        /// <summary>
        /// Retrieves a HashSet<T>.
        /// </summary>
        /// <returns></returns>
        public static HashSet<T> RetrieveHashSet()
        {
            lock (_lock)
            {
                HashSet<T> result;
                if (!_hashSetCache.TryPop(out result))
                    result = new();

                return result;
            }
        }

        /// <summary>
        /// Retrieves a collection adding one entry.
        /// </summary>
        /// <returns></returns>
        public static HashSet<T> RetrieveHashSet(T entry)
        {
            lock (_lock)
            {
                HashSet<T> result;
                if (!_hashSetCache.TryPop(out result))
                    return new();

                result.Add(entry);
                return result;
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array set default, from the beginning.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref T[] value, int count)
        {
            lock (_lock)
            {
                Store(value, count);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        /// <param name = "count">Number of entries in the array from the beginning.</param>
        public static void Store(T[] value, int count)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                for (int i = 0; i < count; i++)
                    value[i] = default;

                if (_arrayCache.Count < MAXIMUM_CACHE_COUNT)
                    _arrayCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref List<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(List<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();

                if (_listCache.Count < MAXIMUM_CACHE_COUNT)
                    _listCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref SortedSet<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(SortedSet<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();

                if (_sortedSetCache.Count < MAXIMUM_CACHE_COUNT)
                    _sortedSetCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref Queue<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(Queue<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();

                if (_queueCache.Count < MAXIMUM_CACHE_COUNT)
                    _queueCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref BasicQueue<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(BasicQueue<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();

                if (_basicQueueCache.Count < MAXIMUM_CACHE_COUNT)
                    _basicQueueCache.Push(value);
            }
        }

        /// <summary>
        /// Stores a collection and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref HashSet<T> value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a collection.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        public static void Store(HashSet<T> value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                value.Clear();

                if (_hashSetCache.Count < MAXIMUM_CACHE_COUNT)
                    _hashSetCache.Push(value);
            }
        }
    }

    /// <summary>
    /// Caches objects of a single generic.
    /// </summary>
    public static class ObjectCaches<T> where T : new()
    {
        /// <summary>
        /// Stack to use.
        /// </summary>
        private static readonly Stack<T> _stack = new();
        /// <summary>
        /// Maximum number of entries allowed for the cache.
        /// </summary>
        private const int MAXIMUM_CACHE_COUNT = 50;
        /// <summary>
        /// Thread lock object.
        /// </summary>
        private static object _lock = new();

        static ObjectCaches()
        {
            /* Initializes lock if not already -- this covers
             * the rare chance a thread other than Unity accesses
             * this class first. */
            if (_lock == null)
                _lock = new();
        }

        // /// <summary>
        // /// Forces _lock to initialize on the Unity main thread.
        // /// </summary>
        // [RuntimeInitializeOnLoadMethod]
        // private static void InitializeLockObject() => _lock = new();

        /// <summary>
        /// Returns a value from the stack or creates an instance when the stack is empty.
        /// </summary>
        /// <returns></returns>
        public static T Retrieve()
        {
            lock (_lock)
            {
                T result;
                if (!_stack.TryPop(out result))
                    result = new(); // Activator.CreateInstance<T>();

                return result;
            }
        }

        /// <summary>
        /// Stores an instance of T and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value">Value to store.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void StoreAndDefault(ref T value)
        {
            lock (_lock)
            {
                Store(value);
                value = default;
            }
        }

        /// <summary>
        /// Stores a value to the stack.
        /// </summary>
        /// <param name = "value"></param>
        public static void Store(T value)
        {
            lock (_lock)
            {
                if (value == null)
                    return;

                if (_stack.Count < MAXIMUM_CACHE_COUNT)
                    _stack.Push(value);
            }
        }
    }
    #endregion
}