using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for Dictionary collections.
    /// </summary>
    public static class DictionaryPool<T0, T1>
    {
        /// <summary>
        /// Stack for TheadLocal Dictionary.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<Dictionary<T0, T1>>> Wrapper;
        /// <summary>
        /// Stack for global Dictionary.
        /// </summary>
        private static readonly Stack<Dictionary<T0, T1>> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static DictionaryPool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)) || typeof(IPoolResettable).IsAssignableFrom(typeof(T1)))
            // {
            //     Logger.LogError(typeof(DictionaryPool<,>), $"[{typeof(T0).Name}] or [{typeof(T1).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }

            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a Dictionary.
        /// </summary>
        /// <returns>A cleared Dictionary collection.</returns>
        public static Dictionary<T0, T1> Rent()
        {
            Dictionary<T0, T1> result;

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
        /// Returns a Dictionary and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref Dictionary<T0, T1> value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Returns a Dictionary.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(Dictionary<T0, T1> value)
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
        /// Flushes the ThreadLocal Dictionary Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<Dictionary<T0, T1>> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out Dictionary<T0, T1> item))
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
