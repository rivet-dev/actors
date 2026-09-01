using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;
using CodeBoost.Types;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for RingBuffer collections.
    /// </summary>
    public static class RingBufferPool<T0>
    {
        /// <summary>
        /// Stack for TheadLocal RingBuffer.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<RingBuffer<T0>>> Wrapper;
        /// <summary>
        /// Stack for global RingBuffer.
        /// </summary>
        private static readonly Stack<RingBuffer<T0>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static RingBufferPool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)))
            // {
            //     Logger.LogError(typeof(RingBufferPool<>), $"[{typeof(T0).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
            //
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a RingBuffer.
        /// </summary>
        /// <returns>A cleared RingBuffer collection.</returns>
        public static RingBuffer<T0> Rent()
        {
            RingBuffer<T0> result;

            if (Wrapper.Value.LocalStack.TryPop(out result))
                return result;

            lock (GlobalStack)
            {
                if (GlobalStack.TryPop(out result))
                    return result;
            }

            return new();
        }

        /// <summary>
        /// Returns a RingBuffer and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref RingBuffer<T0>? value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a RingBuffer.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(RingBuffer<T0>? value)
        {
            if (value is null)
                return;

            value.Clear();

            if (Wrapper.Value.LocalStack.Count < MaximumThreadLocalStackSize)
            {
                Wrapper.Value.LocalStack.Push(value);
                return;
            }

            lock (GlobalStack)
            {
                if (GlobalStack.Count < MaximumGlobalStackSize)
                    GlobalStack.Push(value);
            }

            //If here both stacks are at capacity.
        }

        /// <summary>
        /// Flushes the ThreadLocal RingBuffer Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<RingBuffer<T0>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out RingBuffer<T0>? item))
                {
                    if (GlobalStack.Count < MaximumGlobalStackSize)
                        GlobalStack.Push(item);
                    else
                        break;
                }
            }
        }
    }
}
