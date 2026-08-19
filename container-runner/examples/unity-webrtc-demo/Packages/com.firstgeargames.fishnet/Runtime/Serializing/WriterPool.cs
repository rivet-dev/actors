using FishNet.Managing;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using GameKit.Dependencies.Utilities;

namespace FishNet.Serializing
{
    /// <summary>
    /// Writer which is reused to save on garbage collection and performance.
    /// </summary>
    public sealed class PooledWriter : Writer
    {
        public void Store() => WriterPool.Store(this);
        public void StoreLength() => WriterPool.StoreLength(this);

        [Obsolete("Use Clear instead.")]
        public void ResetState() => Clear();

        [Obsolete("This does not function.")]
        public void InitializeState() { }
    }

    /// <summary>
    /// Collection of PooledWriter. Stores and gets PooledWriter.
    /// </summary>
    public static class WriterPool
    {
        #region Private.
        /// <summary>
        /// Pool of writers where length is the minimum and increased at runtime.
        /// </summary>
        private static readonly Stack<PooledWriter> _pool = new();
        /// <summary>
        /// Pool of writers where length is of minimum key and may be increased at runtime.
        /// </summary>
        private static readonly Dictionary<int, Stack<PooledWriter>> _lengthPool = new();
        #endregion

        #region Const.
        /// <summary>
        /// Length of each bracket when using the length based writer pool.
        /// </summary>
        internal const int LENGTH_BRACKET = 1000;
        #endregion

        /// <summary>
        /// Gets a writer from the pool.
        /// </summary>
        public static PooledWriter Retrieve(NetworkManager networkManager)
        {
            PooledWriter result;
            if (!_pool.TryPop(out result))
                result = new();

            result.Clear(networkManager);
            return result;
        }

        /// Gets a writer from the pool.
        /// </summary>
        public static PooledWriter Retrieve()
        {
            return Retrieve(null);
        }

        /// <summary>
        /// Gets the next writer in the pool of minimum length.
        /// </summary>
        /// <param name = "length">Minimum length the writer buffer must be.</param>
        public static PooledWriter Retrieve(int length)
        {
            return Retrieve(null, length);
        }

        /// <summary>
        /// Gets the next writer in the pool of minimum length.
        /// </summary>
        /// <param name = "length">Minimum length the writer buffer must be.</param>
        public static PooledWriter Retrieve(NetworkManager networkManager, int length)
        {
            /* The index returned will be for writers which have
             * length as a minimum capacity.
             * EG: if length is 1200 / 1000 (length_bracket) result
             * will be index 1. Index 0 will be up to 1000, while
             * index 1 will be up to 2000. */
            int index = GetDictionaryIndex(length);
            Stack<PooledWriter> stack;
            PooledWriter result;
            // There is already one pooled.
            if (_lengthPool.TryGetValue(index, out stack) && stack.TryPop(out result))
            {
                result.Clear(networkManager);
            }
            // Not pooled yet or failed to pop.
            else
            {
                // Get any ol' writer.
                result = Retrieve(networkManager);
                /* Ensure length to fill it's bracket.
                 * Increase index by 1 since 0 index would
                 * just return 0 as the capacity. */
                int requiredCapacity = (index + 1) * LENGTH_BRACKET;
                result.EnsureBufferCapacity(requiredCapacity);
            }

            return result;
        }

        /// <summary>
        /// Returns a writer to the appropriate length pool.
        /// Writers must be a minimum of 1000 bytes in length to be sorted by length.
        /// Writers which do not meet the minimum will be resized to 1000 bytes.
        /// </summary>
        public static void StoreLength(PooledWriter writer)
        {
            int index = GetDictionaryIndex(writer);
            Stack<PooledWriter> stack;
            if (!_lengthPool.TryGetValue(index, out stack))
            {
                stack = new();
                _lengthPool[index] = stack;
            }

            stack.Push(writer);
        }

        /// <summary>
        /// Returns a writer to the pool.
        /// </summary>
        public static void Store(PooledWriter writer)
        {
            _pool.Push(writer);
        }

        /// <summary>
        /// Puts writer back into pool if not null, and nullifies source reference.
        /// </summary>
        public static void StoreAndDefault(ref PooledWriter writer)
        {
            if (writer != null)
            {
                _pool.Push(writer);
                writer = null;
            }
        }

        #region Dictionary indexes.
        /// <summary>
        /// Gets which index to use for length when retrieving a writer.
        /// </summary>
        private static int GetDictionaryIndex(int length)
        {
            /* The index returned will be for writers which have
             * length as a minimum capacity.
             * EG: if length is 1200 / 1000 (length_bracket) result
             * will be index 1. Index 0 will be up to 1000, while
             * index 1 will be up to 2000. So to accomodate 1200
             * length index 1 must be used as 0 has a maximum of 1000. */

            /* Examples if length_bracket is 1000, using floor:
             * 800 / 1000 = 0.
             * 1200 / 1000 = 1.
             * 1000 / 1000 = 1. But has 0 remainder so is reduced by 1, resulting in 0.
             */
            int index = UnityEngine.Mathf.FloorToInt(length / LENGTH_BRACKET);
            if (index > 0 && length % LENGTH_BRACKET == 0)
                index--;

            // UnityEngine.Debug.Log($"Returning length {length} from index {index}");
            return index;
        }

        /// <summary>
        /// Gets which index to use for length when storing a writer.
        /// </summary>
        private static int GetDictionaryIndex(PooledWriter writer)
        {
            int capacity = writer.Capacity;
            /* If capacity is less than 1000 then the writer
             * does not meet the minimum length bracket. This should never
             * be the case unless the user perhaps manually calls this method. */
            if (capacity < LENGTH_BRACKET)
            {
                capacity = LENGTH_BRACKET;
                writer.EnsureBufferCapacity(LENGTH_BRACKET);
            }

            /* Since capacity is set to minimum of length_bracket
             * capacity / length_bracket will always be at least 1.
             *
             * Here are some result examples using floor:
             * 1000 / 1000 = 1.
             * 1200 / 1000 = 1.
             * 2400 / 1000 = 2.
             */
            int index = UnityEngine.Mathf.FloorToInt(capacity / LENGTH_BRACKET);
            /* As mentioned the index will always be a minimum of 1. Because of this
             * we can safely reduce index by 1 and it not be negative.
             * This reduction also ensures the writer ends up in the proper pool.
             * Since index 0 ensures minimum of 1000, 1000-1999 would go there.
             * Just as 2000-2999 would go into 1. */
            index--;

            // UnityEngine.Debug.Log($"Storing capacity {capacity} at index {index}");
            return index;
        }
        #endregion
    }
}