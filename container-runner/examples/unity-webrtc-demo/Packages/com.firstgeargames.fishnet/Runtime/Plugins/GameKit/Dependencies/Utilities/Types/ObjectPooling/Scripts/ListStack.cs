using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.ObjectPooling
{
    public class ListStack<GameObject>
    {
        public ListStack()
        {
            _lastAccessedTime = Time.time;
        }

        #region Public.
        /// <summary>
        /// Number of entries within the stack.
        /// </summary>
        public int Count
        {
            get { return Entries.Count; }
        }
        /// <summary>
        /// Entries within this ListStack.
        /// </summary>
        public List<GameObject> Entries { get; private set; } = new();
        /// <summary>
        /// Time an entry was added. Indexes will always match up with Entries.
        /// </summary>
        public List<float> EntriesAddedTimes { get; private set; } = new();
        #endregion

        #region Private.
        /// <summary>
        /// Last time this ListStack was pushed or popped.
        /// </summary>
        private float _lastAccessedTime = 0f;
        #endregion

        /// <summary>
        /// Returns if this ListStack has been accessed recently.
        /// </summary>
        /// <param name = "threshold"></param>
        /// <returns></returns>
        public bool AccessedRecently(float threshold)
        {
            return Time.time - _lastAccessedTime < threshold;
        }

        /// <summary>
        /// Returns a list of GameObjects which were culled from the stack.
        /// </summary>
        /// <param name = "threshold"></param>
        /// <returns></returns>
        public List<GameObject> Cull(float threshold)
        {
            List<GameObject> results = new();
            float time = Time.time;

            for (int i = 0; i < EntriesAddedTimes.Count; i++)
            {
                if (time - EntriesAddedTimes[i] > threshold)
                    results.Add(Entries[i]);
            }

            if (results.Count > 0)
            {
                Entries.RemoveRange(0, results.Count);
                EntriesAddedTimes.RemoveRange(0, results.Count);
            }

            return results;
        }

        /// <summary>
        /// Push an item to the stack.
        /// </summary>
        /// <param name = "item"></param>
        public void Push(GameObject item)
        {
            _lastAccessedTime = Time.time;
            Entries.Add(item);
            EntriesAddedTimes.Add(_lastAccessedTime);
        }

        /// <summary>
        /// Pop an item from the stack.
        /// </summary>
        /// <returns></returns>
        public GameObject Pop()
        {
            _lastAccessedTime = Time.time;
            if (Entries.Count > 0)
            {
                // Return the last entry as it's cheaper than returning the first.
                int nextIndex = Entries.Count - 1;
                //Set entry then remove from lists.
                GameObject entry = Entries[nextIndex];
                Entries.RemoveAt(nextIndex);
                EntriesAddedTimes.RemoveAt(nextIndex);
                return entry;
            }
            else
            {
                return default;
            }
        }

        /// <summary>
        /// Remove an entry at a specified index.
        /// </summary>
        /// <param name = "index"></param>
        public void Remove(int index)
        {
            _lastAccessedTime = Time.time;
            Entries.RemoveAt(index);
            EntriesAddedTimes.RemoveAt(index);
        }

        /// <summary>
        /// Attempts to remove an item from the entries.
        /// </summary>
        /// <param name = "item"></param>
        /// <returns>True if an item was removed.</returns>
        public bool Remove(GameObject item)
        {
            _lastAccessedTime = Time.time;
            int index = Entries.IndexOf(item);
            if (index == -1)
            {
                return false;
            }
            else
            {
                Entries.RemoveAt(index);
                EntriesAddedTimes.RemoveAt(index);
                return true;
            }
        }

        /// <summary>
        /// Clears the stack; does not destroy items within the stack.
        /// </summary>
        public void Clear()
        {
            _lastAccessedTime = Time.time;
            Entries.Clear();
            EntriesAddedTimes.Clear();
        }
    }
}