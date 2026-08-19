using System;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    /// <summary>
    /// Unity 2022 has a bug where codegen will not compile when referencing a Queue type,
    /// while also targeting .Net as the framework API.
    /// As a work around this class is used for queues instead.
    /// </summary>
    public class BasicQueue<T>
    {
        /// <summary>
        /// Maximum size of the collection.
        /// </summary>
        public int Capacity => Collection.Length;
        /// <summary>
        /// Number of elements in the queue.
        /// </summary>
        public int Count => _written;
        /// <summary>
        /// Collection containing data.
        /// </summary>
        private T[] Collection = new T[4];
        /// <summary>
        /// Current write index of the collection.
        /// </summary>
        public int WriteIndex { get; private set; }
        /// <summary>
        /// Buffer for resizing.
        /// </summary>
        private T[] _resizeBuffer = new T[0];
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
        /// <param name = "data"></param>
        public void Enqueue(T data)
        {
            if (_written == Collection.Length)
                Resize();

            if (WriteIndex >= Collection.Length)
                WriteIndex = 0;
            Collection[WriteIndex] = data;

            WriteIndex++;
            _written++;
        }

        /// <summary>
        /// Tries to dequeue the next entry.
        /// </summary>
        /// <param name = "result">Dequeued entry.</param>
        /// <param name = "defaultArrayEntry">True to set the array entry as default.</param>
        /// <returns>True if an entry existed to dequeue.</returns>
        public bool TryDequeue(out T result, bool defaultArrayEntry = true)
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
        /// <param name = "defaultArrayEntry">True to set the array entry as default.</param>
        public T Dequeue(bool defaultArrayEntry = true)
        {
            if (_written == 0)
                return default;

            T result = Collection[_read];
            if (defaultArrayEntry)
                Collection[_read] = default;

            _written--;
            _read++;
            if (_read >= Collection.Length)
                _read = 0;

            return result;
        }

        /// <summary>
        /// Tries to peek the next entry.
        /// </summary>
        /// <param name = "result">Peeked entry.</param>
        /// <returns>True if an entry existed to peek.</returns>
        public bool TryPeek(out T result)
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
        /// <returns></returns>
        public T Peek()
        {
            if (_written == 0)
                throw new($"Queue of type {typeof(T).Name} is empty.");

            return Collection[_read];
        }

        /// <summary>
        /// Returns an entry at index or default if index is invalid.
        /// </summary>
        public T GetIndexOrDefault(int simulatedIndex)
        {
            int offset = GetRealIndex(simulatedIndex, allowUnusedBuffer: false, log: false);
            if (offset != -1 && offset < Collection.Length)
                return Collection[offset];

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

            DefaultCollection(Collection);
            DefaultCollection(_resizeBuffer);

            void DefaultCollection(T[] array)
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
            T[] resizeBuffer = _resizeBuffer;
            if (resizeBuffer.Length < doubleLength)
                Array.Resize(ref resizeBuffer, doubleLength);
            // Copy from the read of queue first.
            int copyLength = length - read;
            Array.Copy(Collection, read, resizeBuffer, 0, copyLength);
            /* If read index was higher than 0
             * then copy remaining data as well from 0. */
            if (read > 0)
                Array.Copy(Collection, 0, resizeBuffer, copyLength, read);

            // Set _array to resize.
            Collection = resizeBuffer;
            // Reset positions.
            _read = 0;
            WriteIndex = length;
        }

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
                return Collection[offset];
            }
            set
            {
                int offset = GetRealIndex(simulatedIndex);
                Collection[offset] = value;
            }
        }

        /// <summary>
        /// Returns the real index of the collection using a simulated index.
        /// </summary>
        /// <param name = "allowUnusedBuffer">True to allow an index be returned from an unused portion of the buffer so long as it is within bounds.</param>
        private int GetRealIndex(int simulatedIndex, bool allowUnusedBuffer = false, bool log = true)
        {
            if (simulatedIndex >= Capacity)
            {
                return ReturnError();
            }
            else
            {
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
            }

            int ReturnError()
            {
                if (log)
                    Debug.LogError($"Index {simulatedIndex} is out of range. Collection count is {_written}, Capacity is {Capacity}");
                return -1;
            }
        }
    }
}