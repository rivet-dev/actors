using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for List collections.
    /// </summary>
    public static class ListPool<T0>
    {
        /// <summary>
        /// Stack for TheadLocal List.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<List<T0>>> Wrapper;
        /// <summary>
        /// Stack for global List.
        /// </summary>
        private static readonly Stack<List<T0>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static ListPool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)))
            // {
            //     Logger.LogError(typeof(List<>), $"[{typeof(T0).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
                
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a List.
        /// </summary>
        /// <returns>A cleared List collection.</returns>
        public static List<T0> Rent()
        {
            List<T0> result;

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
        /// Returns a List and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref List<T0>? value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a List.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(List<T0>? value)
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
        /// Flushes the ThreadLocal List Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<List<T0>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out List<T0>? item))
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
