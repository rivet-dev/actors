using FishNet.Managing;
using FishNet.Object;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Component.Observing
{
    public class GridEntry
    {
        /// <summary>
        /// Position on the grid.
        /// </summary>
        public Vector2Int Position;
        /// <summary>
        /// This grid entry as well those neighboring it.
        /// </summary>
        public HashSet<GridEntry> NearbyEntries;
        public GridEntry() { }

        public GridEntry(HashSet<GridEntry> nearby)
        {
            NearbyEntries = nearby;
        }

        public void SetValues(Vector2Int position, HashSet<GridEntry> nearby)
        {
            Position = position;
            NearbyEntries = nearby;
        }

        public void SetValues(HashSet<GridEntry> nearby)
        {
            NearbyEntries = nearby;
        }

        public void SetValues(Vector2Int position)
        {
            Position = position;
        }

        public void Reset()
        {
            Position = Vector2Int.zero;
            NearbyEntries.Clear();
        }
    }

    public class HashGrid : MonoBehaviour
    {
        #region Types.
        public enum GridAxes : byte
        {
            XY = 0,
            YZ = 1,
            XZ = 2
        }
        #endregion

        #region Internal.
        /// <summary>
        /// Value for when grid position is not set.
        /// </summary>
        internal static Vector2Int UnsetGridPosition = Vector2Int.one * int.MaxValue;
        /// <summary>
        /// An empty grid entry.
        /// </summary>
        internal static GridEntry EmptyGridEntry = new(new());
        #endregion

        #region Serialized.
        /// <summary>
        /// Axes of world space to base the grid on.
        /// </summary>
        [Tooltip("Axes of world space to base the grid on.")]
        [SerializeField]
        private GridAxes _gridAxes = GridAxes.XY;
        /// <summary>
        /// Accuracy of the grid. Objects will be considered nearby if they are within this number of units. Lower values may be more expensive.
        /// </summary>
        [Tooltip("Accuracy of the grid. Objects will be considered nearby if they are within this number of units. Lower values may be more expensive.")]
        [Range(1, ushort.MaxValue)]
        [SerializeField]
        private ushort _accuracy = 10;
        #endregion

        /// <summary>
        /// Half of accuracy.
        /// </summary>
        private int _halfAccuracy;
        /// <summary>
        /// Cache of List<GridEntry>.
        /// </summary>
        private Stack<HashSet<GridEntry>> _gridEntryHashSetCache = new();
        /// <summary>
        /// Cache of GridEntrys.
        /// </summary>
        private Stack<GridEntry> _gridEntryCache = new();
        /// <summary>
        /// All grid entries.
        /// </summary>
        private Dictionary<Vector2Int, GridEntry> _gridEntries = new();
        /// <summary>
        /// NetworkManager this is used with.
        /// </summary>
        private NetworkManager _networkManager;

        private void Awake()
        {
            _networkManager = GetComponentInParent<NetworkManager>();

            if (_networkManager == null)
            {
                _networkManager.LogError($"NetworkManager not found on object or within parent of {gameObject.name}. The {GetType().Name} must be placed on or beneath a NetworkManager.");
                return;
            }

            // Make sure there is only one per networkmanager.
            if (!_networkManager.HasInstance<HashGrid>())
            {
                _halfAccuracy = Mathf.CeilToInt((float)_accuracy / 2f);
                _networkManager.RegisterInstance(this);
            }
            else
            {
                Destroy(this);
            }
        }

        /// <summary>
        /// Sets out values to be used when creating a new GridEntry.
        /// </summary>
        private void OutputNewGridCollections(out GridEntry gridEntry, out HashSet<GridEntry> gridEntries)
        {
            const int cacheCount = 100;


            if (!_gridEntryHashSetCache.TryPop(out gridEntries))
            {
                BuildGridEntryHashSetCache();
                gridEntries = new();
            }

            if (!_gridEntryCache.TryPop(out gridEntry))
            {
                BuildGridEntryCache();
                gridEntry = new();
            }

            void BuildGridEntryHashSetCache()
            {
                for (int i = 0; i < cacheCount; i++)
                    _gridEntryHashSetCache.Push(new());
            }

            void BuildGridEntryCache()
            {
                for (int i = 0; i < cacheCount; i++)
                    _gridEntryCache.Push(new());
            }
        }

        /// <summary>
        /// Creates a GridEntry for position and inserts it into GridEntries.
        /// </summary>
        private GridEntry CreateGridEntry(Vector2Int position)
        {
            // Make this into a stack that populates a number of entries when empty. also populate with some in awake.
            GridEntry newEntry;
            HashSet<GridEntry> nearby;
            OutputNewGridCollections(out newEntry, out nearby);
            newEntry.SetValues(position, nearby);
            // Add to grid.
            _gridEntries[position] = newEntry;

            // Get neighbors.
            int endX = position.x + 1;
            int endY = position.y + 1;
            int iterations = 0;
            for (int x = position.x - 1; x <= endX; x++)
            {
                for (int y = position.y - 1; y <= endY; y++)
                {
                    iterations++;
                    if (_gridEntries.TryGetValue(new(x, y), out GridEntry foundEntry))
                    {
                        nearby.Add(foundEntry);
                        foundEntry.NearbyEntries.Add(newEntry);
                    }
                }
            }

            return newEntry;
        }

        /// <summary>
        /// Gets grid positions and neighbors for a NetworkObject.
        /// </summary>
        internal void GetNearbyHashGridPositions(NetworkObject nob, ref HashSet<Vector2Int> collection)
        {
            Vector2Int position = GetHashGridPosition(nob);
            // Get neighbors.
            int endX = position.x + 1;
            int endY = position.y + 1;
            for (int x = position.x - 1; x < endX; x++)
            {
                for (int y = position.y - 1; y < endY; y++)
                    collection.Add(new(x, y));
            }
        }

        /// <summary>
        /// Gets the grid position to use for a NetworkObjects current position.
        /// </summary>
        internal Vector2Int GetHashGridPosition(NetworkObject nob)
        {
            Vector3 position = nob.transform.position;
            float fX;
            float fY;
            if (_gridAxes == GridAxes.XY)
            {
                fX = position.x;
                fY = position.y;
            }
            else if (_gridAxes == GridAxes.XZ)
            {
                fX = position.x;
                fY = position.z;
            }
            else if (_gridAxes == GridAxes.YZ)
            {
                fX = position.y;
                fY = position.z;
            }
            else
            {
                _networkManager.LogError($"GridAxes of {_gridAxes.ToString()} is not handled.");
                return default;
            }

            return new((int)fX / _halfAccuracy, (int)fY / _halfAccuracy);
        }

        /// <summary>
        /// Gets a GridEntry for a NetworkObject, creating the entry if needed.
        /// </summary>
        internal GridEntry GetGridEntry(NetworkObject nob)
        {
            Vector2Int pos = GetHashGridPosition(nob);
            return GetGridEntry(pos);
        }

        /// <summary>
        /// Gets a GridEntry for position, creating the entry if needed.
        /// </summary>
        internal GridEntry GetGridEntry(Vector2Int position)
        {
            GridEntry result;
            if (!_gridEntries.TryGetValue(position, out result))
                result = CreateGridEntry(position);

            return result;
        }
    }
}