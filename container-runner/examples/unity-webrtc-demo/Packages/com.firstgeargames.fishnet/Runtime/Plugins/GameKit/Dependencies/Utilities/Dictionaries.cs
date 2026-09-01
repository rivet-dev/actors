using System.Collections.Generic;

namespace GameKit.Dependencies.Utilities
{
    public static class DictionaryFN
    {

        /// <summary>
        /// Uses a hacky way to TryGetValue on a dictionary when using IL2CPP and on mobile.
        /// This is to support older devices that don't properly handle IL2CPP builds.
        /// </summary>
        public static bool TryGetValueIL2CPP<TKey, TValue>(this IReadOnlyDictionary<TKey, TValue> dict, TKey key, out TValue value)
        {
            #if ENABLE_IL2CPP && UNITY_IOS || UNITY_ANDROID
            if (dict.ContainsKey(key))
            {
                value = dict[key];
                return true;
            }

                value = default;
                return false;
            #else
            return dict.TryGetValue(key, out value);
            #endif
        }

     
        /// <summary>
        /// Returns values as a list.
        /// </summary>
        /// <returns></returns>
        public static List<TValue> ValuesToList<TKey, TValue>(this IReadOnlyDictionary<TKey, TValue> dict, bool useCache)
        {
            List<TValue> result = useCache ? CollectionCaches<TValue>.RetrieveList() : new(dict.Count);

            //No need to clear the list since it's already clear.
            dict.ValuesToList(ref result, clearLst: false);

            return result;
        }

        /// <summary>
        /// Adds values to a list.
        /// </summary>
        public static void ValuesToList<TKey, TValue>(this IReadOnlyDictionary<TKey, TValue> dict, ref List<TValue> result, bool clearLst)
        {
            if (clearLst)
                result.Clear();

            foreach (TValue item in dict.Values)
                result.Add(item);
        }

        /// <summary>
        /// Returns keys as a list.
        /// </summary>
        public static List<TKey> KeysToList<TKey, TValue>(this IReadOnlyDictionary<TKey, TValue> dict, bool useCache)
        {
            List<TKey> result = useCache ? CollectionCaches<TKey>.RetrieveList() : new(dict.Count);

            //No need to clear the list since it's already clear.
            dict.KeysToList(ref result, clearLst: false);

            return result;
        }

        /// <summary>
        /// Adds keys to a list.
        /// </summary>
        public static void KeysToList<TKey, TValue>(this IReadOnlyDictionary<TKey, TValue> dict, ref List<TKey> result, bool clearLst)
        {
            result.Clear();

            foreach (TKey item in dict.Keys)
                result.Add(item);
        }
    }
}