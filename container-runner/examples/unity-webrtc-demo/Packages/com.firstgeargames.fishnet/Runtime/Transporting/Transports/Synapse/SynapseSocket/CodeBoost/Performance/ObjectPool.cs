using System.Collections.Generic;
using System.Threading;
using CodeBoost.Extensions;
using CodeBoost.Logging;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for generic objects.
    /// </summary>
    public static class ObjectPool<T0> where T0 : new()
    {
        /// <summary>
        /// Stack for TheadLocal Object.
        /// </summary>
        private static readonly ThreadLocal<ThreadLocalStackWrapper<T0>> Wrapper;
        /// <summary>
        /// Stack for global Object.
        /// </summary>
        private static readonly Stack<T0> GlobalStack = new();
        /// <summary>
        /// Maximum number of entries allowed in the global stack.
        /// </summary>
        private const int MaximumGlobalStackSize = 200;
        /// <summary>
        /// Maximum number of entries allowed in the ThreadLocal stack.
        /// </summary>
        private const int MaximumThreadLocalStackSize = 100;

        static ObjectPool()
        {
            // if (typeof(IPoolResettable).IsAssignableFrom(typeof(T0)))
            // {
            //     Logger.LogError(typeof(ObjectPool<>), $"[{typeof(T0).Name}] implements IPoolResettable; use the Resettable pool instead.");
            //     return;
            // }
                
            Wrapper = new(valueFactory: () => new(Flush), trackAllValues: false);
        }

        /// <summary>
        /// Rents a generic object.
        /// </summary>
        /// <returns>A new or pooled instance of T0.</returns>
        public static T0 Rent()
        {
            T0 result;

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
        /// Returns a generic object and sets the provided reference to null;
        /// This Method will not execute if the value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref T0? value)
        {
            Return(value);

            value = default;
        }

        /// <summary>
        /// Returns a generic object.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void Return(T0? value)
        {
            if (value is null)
                return;

            // Note: If T0 implements an interface like IResettable, 
            // you would call value.Clear() or value.Reset() here.

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
        /// Flushes the ThreadLocal Object Stack into the global Stack.
        /// </summary>
        private static void Flush(Stack<T0> localStack)
        {
            if (localStack.Count == 0)
                return;

            lock (GlobalStack)
            {
                while (localStack.TryPop(out T0? item))
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
