using System;
using System.Buffers;
using System.Collections;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types
{
    /// <summary>
    /// Writes values to a collection of a set size, overwriting old values as needed.
    /// </summary>
    public class RingBuffer<T> : IEnumerable<T>
    {
        #region Types.
        /// <summary>
        /// Custom enumerator to prevent garbage collection.
        /// </summary>
        public struct Enumerator : IEnumerator<T>
        {
            #region Public.
            /// <summary>
            /// Current entry in the enumerator.
            /// </summary>
            public T Current { get; private set; }
            #endregion

            #region Private.
            /// <summary>
            /// RollingCollection to use.
            /// </summary>
            private RingBuffer<T> _enumeratedRingBuffer;
            /// <summary>
            /// Collection to iterate.
            /// </summary>
            private T[] _collection;
            /// <summary>
            /// Number of entries read during the enumeration.
            /// </summary>
            private int _entriesEnumerated;
            /// <summary>
            /// Start index of enumerations.
            /// </summary>
            private int _startIndex;
            /// <summary>
            /// True if currently enumerating.
            /// </summary>
            private bool _enumerating => _enumeratedRingBuffer != null;
            /// <summary>
            /// Count of the collection during initialization.
            /// </summary>
            private int _initializeCollectionCount;
            #endregion

            public void Initialize(RingBuffer<T> c)
            {
                // if none are written then return.
                if (c.Count == 0)
                    return;

                _entriesEnumerated = 0;
                _startIndex = c.GetRealIndex(0);
                _enumeratedRingBuffer = c;
                _collection = c.Collection;
                _initializeCollectionCount = c.Count;
                Current = default;
            }

            public bool MoveNext()
            {
                if (!_enumerating)
                    return false;

                int written = _enumeratedRingBuffer.Count;

                if (written != _initializeCollectionCount)
                {
                    Debug.LogError($"{_enumeratedRingBuffer.GetType().Name} collection was modified during enumeration.");
                    // This will force a return/reset.
                    _entriesEnumerated = written;
                }

                if (_entriesEnumerated >= written)
                {
                    Reset();
                    return false;
                }

                int index = _startIndex + _entriesEnumerated;
                int capacity = _enumeratedRingBuffer.Capacity;
                if (index >= capacity)
                    index -= capacity;
                Current = _collection[index];

                _entriesEnumerated++;

                return true;
            }

            /// <summary>
            /// Resets read count.
            /// </summary>
            public void Reset()
            {
                /* Only need to reset value types.
                 * Numeric types change during initialization. */
                _enumeratedRingBuffer = default;
                _collection = default;
                Current = default;
            }

            object IEnumerator.Current => Current;
            public void Dispose() { }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Current write index of the collection.
        /// </summary>
        public int WriteIndex { get; private set; }
        /// <summary>
        /// Number of entries currently written.
        /// </summary>
        public int Count => _written;
        /// <summary>
        /// Maximum size of the collection.
        /// </summary>
        public int Capacity;
        /// <summary>
        /// Collection being used.
        /// </summary>
        public T[] Collection = new T[0];
        /// <summary>
        /// True if initialized.
        /// </summary>
        public bool Initialized { get; private set; }
        #endregion

        #region Private.
        /// <summary>
        /// Number of entries written. This will never go beyond the capacity but will be less until capacity is filled.
        /// </summary>
        private int _written;
        /// <summary>
        /// Enumerator for the collection.
        /// </summary>
        private Enumerator _enumerator;
        #endregion

        #region Consts.
        /// <summary>
        /// Default capacity when none is psecified.
        /// </summary>
        public const int DEFAULT_CAPACITY = 60;
        #endregion

        /// <summary>
        /// Initializes with default capacity.
        /// </summary>
        public RingBuffer()
        {
            Initialize(DEFAULT_CAPACITY);
        }

        /// <summary>
        /// Initializes with a set capacity.
        /// </summary>
        /// <param name = "capacity">Size to initialize the collection as. This cannot be changed after initialized.</param>
        public RingBuffer(int capacity)
        {
            Initialize(capacity);
        }

        /// <summary>
        /// Initializes the collection at length.
        /// </summary>
        /// <param name = "capacity">Size to initialize the collection as. This cannot be changed after initialized.</param>
        public void Initialize(int capacity)
        {
            if (capacity <= 0)
            {
                Debug.LogError($"Collection length must be larger than 0.");
                return;
            }

            if (Collection == null)
            {
                GetNewCollection();
            }
            else if (Collection.Length < capacity)
            {
                Clear();
                ArrayPool<T>.Shared.Return(Collection);
                GetNewCollection();
            }
            else
            {
                Clear();
            }

            Capacity = capacity;
            Initialized = true;

            void GetNewCollection() => Collection = ArrayPool<T>.Shared.Rent(capacity);
        }

        /// <summary>
        /// Initializes with default capacity.
        /// </summary>
        /// <param name = "log">True to log automatic initialization.</param>
        public void Initialize()
        {
            if (!Initialized)
            {
                Debug.Log($"RingBuffer for type {typeof(T).FullName} is being initialized with a default capacity of {DEFAULT_CAPACITY}.");
                Initialize(DEFAULT_CAPACITY);
            }
        }

        /// <summary>
        /// Clears the collection to default values and resets indexing.
        /// </summary>
        public void Clear()
        {
            for (int i = 0; i < Capacity; i++)
                Collection[i] = default;

            _written = 0;
            WriteIndex = 0;
            _enumerator.Reset();
        }

        /// <summary>
        /// Resets the collection without clearing.
        /// </summary>
        [Obsolete("This method no longer functions. Use Clear() instead.")] // Remove on V5
        public void Reset() { }

        /// <summary>
        /// Inserts an entry into the collection.
        /// This is can be an expensive operation on larger buffers.
        /// </summary>
        /// <param name = "simulatedIndex">Simulated index to return. A value of 0 would return the first simulated index in the collection.</param>
        /// <param name = "data">Data to insert.</param>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public T Insert(int simulatedIndex, T data)
        {
            Initialize();

            int written = _written;
            // If simulatedIndex is 0 and none are written then add.
            if (simulatedIndex == 0 && written == 0)
                return Add(data);

            int realIndex = GetRealIndex(simulatedIndex);
            if (realIndex == -1)
                return default;


            // If adding to the end or none written.
            if (simulatedIndex == written - 1)
                return Add(data);

            int lastSimulatedIndex = written == Capacity ? written - 1 : written;

            while (lastSimulatedIndex > simulatedIndex)
            {
                int lRealIndex = GetRealIndex(lastSimulatedIndex, true);
                int lPrevRealIndex = GetRealIndex(lastSimulatedIndex - 1);
                Collection[lRealIndex] = Collection[lPrevRealIndex];
                lastSimulatedIndex--;
            }

            T prev = Collection[realIndex];
            Collection[realIndex] = data;
            // If written was not maxed out then increase it.
            if (written < Capacity)
                IncreaseWritten();

            return prev;
        }

        /// <summary>
        /// Adds an entry to the collection, returning a replaced entry.
        /// </summary>
        /// <param name = "data">Data to add.</param>
        /// <returns>Replaced entry. Value will be default if no entry was replaced.</returns>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public T Add(T data)
        {
            Initialize();

            T current = Collection[WriteIndex];

            Collection[WriteIndex] = data;
            IncreaseWritten();

            return current;
        }

        /// <summary>
        /// Returns the first entry and removes it from the buffer.
        /// </summary>
        /// <returns></returns>
        public T Dequeue()
        {
            if (_written == 0)
                return default;

            int offset = GetRealIndex(0);
            T result = Collection[offset];

            RemoveRange(fromStart: true, 1);
            return result;
        }

        /// <summary>
        /// Returns if able to dequeue an entry and removes it from the buffer if so.
        /// </summary>
        /// <returns></returns>
        public bool TryDequeue(out T result)
        {
            if (_written == 0)
            {
                result = default;
                return false;
            }

            int offset = GetRealIndex(0);
            result = Collection[offset];

            RemoveRange(fromStart: true, 1);
            return true;
        }

        /// <summary>
        /// Adds an entry to the collection, returning a replaced entry.
        /// This method internally redirects to add.
        /// </summary>
        public T Enqueue(T data) => Add(data);

        /// <summary>
        /// Returns value in actual index as it relates to simulated index.
        /// </summary>
        /// <param name = "simulatedIndex">Simulated index to return. A value of 0 would return the first simulated index in the collection.</param>
        /// <returns></returns>
        public T this[int simulatedIndex]
        {
            get
            {
                int offset = GetRealIndex(simulatedIndex);
                if (offset >= 0)
                    return Collection[offset];
                else
                    return default;
            }
            set
            {
                int offset = GetRealIndex(simulatedIndex);
                if (offset >= 0)
                    Collection[offset] = value;
            }
        }

        /// <summary>
        /// Increases written count and handles offset changes.
        /// </summary>
        private void IncreaseWritten()
        {
            int capacity = Capacity;

            WriteIndex++;
            _written++;
            //If index would exceed next iteration reset it.
            if (WriteIndex >= capacity)
                WriteIndex = 0;

            /* If written has exceeded capacity
             * then the start index needs to be moved
             * to adjust for overwritten values. */
            if (_written > capacity)
                _written = capacity;
        }

        /// <summary>
        /// Returns the real index of the collection using a simulated index.
        /// </summary>
        /// <param name = "allowUnusedBuffer">True to allow an index be returned from an unused portion of the buffer so long as it is within bounds.</param>
        private int GetRealIndex(int simulatedIndex, bool allowUnusedBuffer = false)
        {
            if (simulatedIndex >= Capacity)
            {
                return ReturnError();
            }
            else
            {
                int written = _written;
                //May be out of bounds if allowUnusedBuffer is false.
                if (simulatedIndex >= written)
                {
                    if (!allowUnusedBuffer)
                        return ReturnError();
                }
                int offset = Capacity - written + simulatedIndex + WriteIndex;
                if (offset >= Capacity)
                    offset -= Capacity;

                return offset;
            }

            int ReturnError()
            {
                Debug.LogError($"Index {simulatedIndex} is out of range. Written count is {_written}, Capacity is {Capacity}");
                return -1;
            }
        }

        /// <summary>
        /// Removes values from the simulated start of the collection.
        /// </summary>
        /// <param name = "fromStart">True to remove from the start, false to remove from the end.</param>
        /// <param name = "length">Number of entries to remove.</param>
        public void RemoveRange(bool fromStart, int length)
        {
            if (length == 0)
                return;
            if (length < 0)
            {
                Debug.LogError($"Negative values cannot be removed.");
                return;
            }
            //Full reset if value is at or more than written.
            if (length >= _written)
            {
                Clear();
                return;
            }

            _written -= length;
            if (fromStart)
            {
                //No steps are needed from start other than reduce written, which is done above.
            }
            else
            {
                WriteIndex -= length;
                if (WriteIndex < 0)
                    WriteIndex += Capacity;
            }
        }

        /// <summary>
        /// Returns Enumerator for the collection.
        /// </summary>
        /// <returns></returns>
        public Enumerator GetEnumerator()
        {
            Initialize();
            _enumerator.Initialize(this);
            return _enumerator;
        }

        IEnumerator<T> IEnumerable<T>.GetEnumerator() => GetEnumerator();
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
}