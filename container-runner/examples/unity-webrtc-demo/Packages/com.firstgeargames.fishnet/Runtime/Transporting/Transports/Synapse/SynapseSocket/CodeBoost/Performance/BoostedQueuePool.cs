using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;
using CodeBoost.Types;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for BoostedQueue collections.
    /// </summary>
    public static class BoostedQueuePool<T0>
    {
        /// <summary>
        /// Stack for TheadLocal BoostedQueue.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<BoostedQueue<T0>>> Wrapper;
        /// <summary>
        /// Stack for global BoostedQueue.
        /// </summary>
        private static readonly Stack<BoostedQueue<T0>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static BoostedQueuePool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)))
            // {
            //     Logger.LogError(typeof(BoostedQueuePool<>), $"[{typeof(T0).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
            //
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a BoostedQueue.
        /// </summary>
        /// <returns>A cleared BoostedQueue collection.</returns>
        public static BoostedQueue<T0> Rent()
        {
            BoostedQueue<T0> result;

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
        /// Returns a BoostedQueue and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref BoostedQueue<T0>? value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a BoostedQueue.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(BoostedQueue<T0>? value)
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
        /// Flushes the ThreadLocal BoostedQueue Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<BoostedQueue<T0>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out BoostedQueue<T0>? item))
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
