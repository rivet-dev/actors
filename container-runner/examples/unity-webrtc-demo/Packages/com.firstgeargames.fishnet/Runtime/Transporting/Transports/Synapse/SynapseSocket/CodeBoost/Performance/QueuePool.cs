using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for Queue collections.
    /// </summary>
    public static class QueuePool<T0>
    {
        /// <summary>
        /// Stack for TheadLocal Queue.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<Queue<T0>>> Wrapper;
        /// <summary>
        /// Stack for global Queue.
        /// </summary>
        private static readonly Stack<Queue<T0>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static QueuePool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)))
            // {
            //     Logger.LogError(typeof(QueuePool<>), $"[{typeof(T0).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
                
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a Queue.
        /// </summary>
        /// <returns>A cleared Queue collection.</returns>
        public static Queue<T0> Rent()
        {
            Queue<T0> result;

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
        /// Returns a Queue and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref Queue<T0>? value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a Queue.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(Queue<T0>? value)
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
        /// Flushes the ThreadLocal Queue Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<Queue<T0>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out Queue<T0>? item))
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
