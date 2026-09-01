using GameKit.Dependencies.Utilities.Types;
using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public interface IWeighted
    {
        float GetWeight();
        ByteRange GetQuantity();
    }

    public static class WeightedRandom
    {
        /// <summary>
        /// Gets random entries by weight.
        /// </summary>
        /// <param name = "source">Entries to pull from.</param>
        /// <param name = "countRange">Number of entries to get.</param>
        /// <param name = "results">Results of entries. Key is the entry, Value is the number of drops.</param>
        /// <param name = "allowRepeatingDrops">True to allow the same entry to be included within results more than once.</param>
        public static void GetEntries<T>(List<T> source, IntRange countRange, ref Dictionary<T, uint> results, bool allowRepeatingDrops = false) where T : IWeighted
        {
            if (source == null || source.Count == 0)
            {
                Debug.Log($"Source list of type {typeof(T).Name} cannot be null or empty.");
                return;
            }

            int count = Ints.RandomInclusiveRange(countRange.Minimum, countRange.Maximum);
            // If to not return any then exit early.
            if (count == 0)
                return;

            // Number of times each item has dropped.
            Dictionary<T, byte> dropCount = CollectionCaches<T, byte>.RetrieveDictionary();

            // Get the total weight.
            float totalWeight = 0f;
            for (int i = 0; i < source.Count; i++)
                totalWeight += source[i].GetWeight();

            // Make a copy of source to not modify source.
            List<T> sourceCopy = CollectionCaches<T>.RetrieveList();
            foreach (T item in source)
                sourceCopy.Add(item);

            while (results.Count < count)
            {
                int startCount = results.Count;
                /* Reset copy to totalWeight.
                 * totalWeight will be modified if
                 * a non-repeatable item is pulled. */
                float tWeightCopy = totalWeight;
                float rnd = Random.Range(0f, totalWeight);

                for (int i = 0; i < sourceCopy.Count; i++)
                {
                    T item = sourceCopy[i];
                    float weight = item.GetWeight();

                    if (rnd <= weight)
                    {
                        // Try to get current count.
                        results.TryGetValueIL2CPP(item, out uint currentCount);
                        // Set new vlaue.
                        results[item] = currentCount + 1;
                        /* If cannot stay in collection then remove it
                         * from copy and remove its weight
                         * from total. */
                        if (!allowRepeatingDrops)
                        {
                            sourceCopy.RemoveAt(i);
                            totalWeight -= weight;
                        }
                        break;
                    }
                    else
                    {
                        tWeightCopy -= weight;
                    }
                }

                /* If nothing was added to results then
                 * something went wrong. */
                if (results.Count == startCount)
                    break;
            }

            CollectionCaches<T, byte>.Store(dropCount);
        }
    }
}