using CodeBoost.Logging;
using System;
#pragma warning disable CS8603 // Possible null reference return.
#pragma warning disable CS8601 // Possible null reference assignment.


namespace CodeBoost.Types
{

    /// <summary>
    /// Unity 2022 has a bug where codegen will not compile when referencing a Queue type,
    /// while also targeting .Net as the framework API.
    /// As a work around this class is used for queues instead.
    /// </summary>
    public class BoostedQueue<T0>
    {
        /// <summary>
        /// Maximum size of the collection.
        /// </summary>
        public int Capacity => _collection.Length;
        /// <summary>
        /// Number of elements in the queue.
        /// </summary>
        public int Count => _written;
        /// <summary>
        /// Collection containing data.
        /// </summary>
        private T0[] _collection = new T0[4];
        /// <summary>
        /// Current write index of the collection.
        /// </summary>
        public int WriteIndex { get; private set; }
        /// <summary>
        /// Buffer for resizing.
        /// </summary>
        private readonly T0[] _resizeBuffer = new T0[0];
        /// <summary>
        /// Read position of the next Dequeue.
        /// </summary>
        private int _read;
        /// <summary>
        /// Length of the queue.
        /// </summary>
        private int _written;

        /// <summary>
        /// Enqueues an entry.
        /// </summary>
        /// <param name = "data"> </param>
        public void Enqueue(T0 data)
        {
            if (_written == _collection.Length)
                Resize();

            if (WriteIndex >= _collection.Length)
                WriteIndex = 0;
            _collection[WriteIndex] = data;

            WriteIndex++;
            _written++;
        }

        /// <summary>
        /// Tries to dequeue the next entry.
        /// </summary>
        /// <param name = "result"> Dequeued entry. </param>
        /// <param name = "defaultArrayEntry"> True to set the array entry as default. </param>
        /// <returns> True if an entry existed to dequeue. </returns>
        public bool TryDequeue(out T0 result, bool defaultArrayEntry = true)
        {
            if (_written == 0)
            {
                result = default;
                return false;
            }

            result = Dequeue(defaultArrayEntry);
            return true;
        }

        /// <summary>
        /// Dequeues the next entry.
        /// </summary>
        /// <param name = "defaultArrayEntry"> True to set the array entry as default. </param>
        public T0 Dequeue(bool defaultArrayEntry = true)
        {
            if (_written == 0)
                return default;

            T0 result = _collection[_read];
            if (defaultArrayEntry)
                _collection[_read] = default;

            _written--;
            _read++;
            if (_read >= _collection.Length)
                _read = 0;

            return result;
        }

        /// <summary>
        /// Tries to peek the next entry.
        /// </summary>
        /// <param name = "result"> Peeked entry. </param>
        /// <returns> True if an entry existed to peek. </returns>
        public bool TryPeek(out T0 result)
        {
            if (_written == 0)
            {
                result = default;
                return false;
            }

            result = Peek();
            return true;
        }

        /// <summary>
        /// Peeks the next queue entry.
        /// </summary>
        /// <returns> </returns>
        public T0 Peek()
        {
            if (_written == 0)
                throw new($"Queue of type {typeof(T0).Name} is empty.");

            return _collection[_read];
        }

        /// <summary>
        /// Returns an entry at index or default if index is invalid.
        /// </summary>
        public T0 GetIndexOrDefault(int simulatedIndex)
        {
            int offset = GetRealIndex(simulatedIndex, allowUnusedBuffer: false, log: false);
            if (offset != -1 && offset < _collection.Length)
                return _collection[offset];

            return default;
        }

        /// <summary>
        /// Clears the queue.
        /// </summary>
        public void Clear()
        {
            _read = 0;
            WriteIndex = 0;
            _written = 0;

            DefaultCollection(_collection);
            DefaultCollection(_resizeBuffer);

            void DefaultCollection(T0[] array)
            {
                int count = array.Length;
                for (int i = 0; i < count; i++)
                    array[i] = default;
            }
        }

        /// <summary>
        /// Doubles the queue size.
        /// </summary>
        private void Resize()
        {
            int length = _written;
            int doubleLength = length * 2;
            int read = _read;

            /* Make sure copy array is the same size as current
             * and copy contents into it. */
            // Ensure large enough to fit contents.
            T0[] resizeBuffer = _resizeBuffer;
            if (resizeBuffer.Length < doubleLength)
                Array.Resize(ref resizeBuffer, doubleLength);
            // Copy from the read of queue first.
            int copyLength = length - read;
            Array.Copy(_collection, read, resizeBuffer, 0, copyLength);
            /* If read index was higher than 0
             * then copy remaining data as well from 0. */
            if (read > 0)
                Array.Copy(_collection, 0, resizeBuffer, copyLength, read);

            // Set _array to resize.
            _collection = resizeBuffer;
            // Reset positions.
            _read = 0;
            WriteIndex = length;
        }

        /// <summary>
        /// Returns value in actual index as it relates to simulated index.
        /// </summary>
        /// <param name = "simulatedIndex"> Simulated index to return. A value of 0 would return the first simulated index in the collection. </param>
        /// <returns> </returns>
        public T0 this[int simulatedIndex]
        {
            get
            {
                int offset = GetRealIndex(simulatedIndex);
                return _collection[offset];
            }
            set
            {
                int offset = GetRealIndex(simulatedIndex);
                _collection[offset] = value;
            }
        }

        /// <summary>
        /// Returns the real index of the collection using a simulated index.
        /// </summary>
        /// <param name = "allowUnusedBuffer"> True to allow an index be returned from an unused portion of the buffer so long as it is within bounds. </param>
        private int GetRealIndex(int simulatedIndex, bool allowUnusedBuffer = false, bool log = true)
        {
            if (simulatedIndex >= Capacity)
            {
                return ReturnError();
            }

            int written = _written;
            // May be out of bounds if allowUnusedBuffer is false.
            if (simulatedIndex >= written)
            {
                if (!allowUnusedBuffer)
                    return ReturnError();
            }

            int offset = Capacity - written + simulatedIndex + WriteIndex;
            if (offset >= Capacity)
                offset -= Capacity;

            return offset;

            int ReturnError()
            {
                if (log)
                    Logger<BoostedQueue<T0>>.LogError($"Index {simulatedIndex} is out of range. Collection count is {_written}, Capacity is {Capacity}");
                return -1;
            }
        }
    }
}
