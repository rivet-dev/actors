using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for SortedList collections.
    /// </summary>
    public static class SortedListPool<T0, T1>
    {
        /// <summary>
        /// Stack for TheadLocal SortedList.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<SortedList<T0, T1>>> Wrapper;
        /// <summary>
        /// Stack for global SortedList.
        /// </summary>
        private static readonly Stack<SortedList<T0, T1>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static SortedListPool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)) || typeof(IPoolResettable).IsAssignableFrom(typeof(T1)))
            // {
            //     Logger.LogError(typeof(SortedListPool<,>), $"[{typeof(T0).Name}] or [{typeof(T1).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
                
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a SortedList.
        /// </summary>
        /// <returns>A cleared SortedList collection.</returns>
        public static SortedList<T0, T1> Rent()
        {
            SortedList<T0, T1> result;

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
        /// Returns a SortedList and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref SortedList<T0, T1>? value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a SortedList.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(SortedList<T0, T1>? value)
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
        /// Flushes the ThreadLocal SortedList Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<SortedList<T0, T1>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out SortedList<T0, T1>? item))
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
