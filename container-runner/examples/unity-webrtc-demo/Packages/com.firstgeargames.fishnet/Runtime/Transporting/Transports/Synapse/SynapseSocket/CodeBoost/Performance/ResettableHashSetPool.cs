using System.Collections.Generic;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a HashSet which is resettable.
    /// </summary>
    public static class ResettableHashSetPool<T0> where T0 : IPoolResettable, new()
    {
        /// <summary>
        /// Retrieves an instance of HashSet.
        /// </summary>
        public static HashSet<T0> Rent() => HashSetPool<T0>.Rent();

        /// <summary>
        /// Stores an instance of HashSet and sets the original reference to null.
        /// </summary>
        public static void ReturnAndNullifyReference(ref HashSet<T0>? value, PoolReturnType collectionReturnType)
        {
            Return(value, collectionReturnType);

            value = null;
        }

        /// <summary>
        /// Stores an instance of HashSet.
        /// </summary>
        public static void Return(HashSet<T0>? value, PoolReturnType collectionReturnType)
        {
            if (value is null)
                return;

            foreach (T0 item in value)
                item?.OnReturn();

            if (collectionReturnType is PoolReturnType.Return)
                HashSetPool<T0>.Return(value);
        }
    }
}
