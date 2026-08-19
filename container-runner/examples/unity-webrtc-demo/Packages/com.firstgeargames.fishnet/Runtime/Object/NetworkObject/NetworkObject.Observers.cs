using FishNet.Component.Observing;
using FishNet.Connection;
using FishNet.Managing;
using FishNet.Managing.Timing;
using FishNet.Observing;
using System;
using System.Collections.Generic;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// Called when the clientHost gains or loses visibility of this object.
        /// Boolean value will be true if clientHost has visibility.
        /// </summary>
        public event HostVisibilityUpdatedDelegate OnHostVisibilityUpdated;

        /// <summary>
        /// </summary>
        /// <param name = "prevVisible">True if clientHost was known to have visibility of the object prior to this invoking.</param>
        /// <param name = "nextVisible">True if the clientHost now has visibility of the object.</param>
        public delegate void HostVisibilityUpdatedDelegate(bool prevVisible, bool nextVisible);

        /// <summary>
        /// Called when this NetworkObject losses all observers or gains observers while previously having none.
        /// </summary>
        public event Action<NetworkObject> OnObserversActive;
        /// <summary>
        /// NetworkObserver on this object.
        /// </summary>
        [HideInInspector]
        public NetworkObserver NetworkObserver = null;
        /// <summary>
        /// Clients which can see and get messages from this NetworkObject.
        /// </summary>
        [HideInInspector]
        public HashSet<NetworkConnection> Observers = new();
        #endregion

        #region Internal.
        /// <summary>
        /// Current HashGrid entry this belongs to.
        /// </summary>
        internal GridEntry HashGridEntry;
        /// <summary>
        /// Last tick an observer was added.
        /// </summary>
        internal uint ObserverAddedTick = TimeManager.UNSET_TICK;
        #endregion

        #region Private.
        /// <summary>
        /// True if NetworkObserver has been initialized.
        /// </summary>
        private bool _networkObserverInitiliazed = false;
        /// <summary>
        /// Found renderers on the NetworkObject and it's children. This is only used as clientHost to hide non-observers objects.
        /// </summary>
        [NonSerialized]
        private List<Renderer> _renderers;
        /// <summary>
        /// True if renderers have been looked up.
        /// </summary>
        private bool _renderersPopulated;
        /// <summary>
        /// Last visibility value for clientHost on this object.
        /// </summary>
        private bool _lastClientHostVisibility;
        /// <summary>
        /// HashGrid for this object.
        /// </summary>
        private HashGrid _hashGrid;
        /// <summary>
        /// Next time this object may update it's position for HashGrid.
        /// </summary>
        private float _nextHashGridUpdateTime;
        /// <summary>
        /// True if this gameObject is static.
        /// </summary>
        private bool _isStatic;
        /// <summary>
        /// Current grid position.
        /// </summary>
        private Vector2Int _hashGridPosition = HashGrid.UnsetGridPosition;
        #endregion

        /// <summary>
        /// Updates Objects positions in the HashGrid for this Networkmanager.
        /// </summary>
        internal void UpdateForNetworkObject(bool force)
        {
            if (_hashGrid == null)
                return;
            if (_isStatic)
                return;

            float unscaledTime = Time.unscaledTime;
            // Not enough time has passed to update.
            if (!force && unscaledTime < _nextHashGridUpdateTime)
                return;

            const float updateInterval = 1f;
            _nextHashGridUpdateTime = unscaledTime + updateInterval;
            Vector2Int newPosition = _hashGrid.GetHashGridPosition(this);
            if (newPosition != _hashGridPosition)
            {
                _hashGridPosition = newPosition;
                HashGridEntry = _hashGrid.GetGridEntry(newPosition);
            }
        }

        /// <summary>
        /// Updates cached renderers used to managing clientHost visibility.
        /// </summary>
        /// <param name = "updateVisibility">True to also update visibility if clientHost.</param>
        public void UpdateRenderers(bool updateVisibility = true)
        {
            InitializeRendererCollection(force: true, updateVisibility);
        }

        /// <summary>
        /// Sets the renderer visibility for clientHost.
        /// </summary>
        /// <param name = "visible">True if renderers are to be visibile.</param>
        /// <param name = "force">True to skip blocking checks.</param>
        public void SetRenderersVisible(bool visible, bool force = false)
        {
            if (!force && !NetworkObserver.UpdateHostVisibility)
                return;

            UpdateRenderVisibility(visible);
        }

        /// <summary>
        /// Updates visibilites on renders without checks.
        /// </summary>
        /// <param name = "visible"></param>
        private void UpdateRenderVisibility(bool visible)
        {
            InitializeRendererCollection(force: false, updateVisibility: false);

            List<Renderer> rs = _renderers;
            for (int i = 0; i < rs.Count; i++)
            {
                Renderer r = rs[i];
                if (r == null)
                {
                    _renderers.RemoveAt(i);
                    i--;
                }
                else
                {
                    r.enabled = visible;
                }
            }

            if (OnHostVisibilityUpdated != null)
                OnHostVisibilityUpdated.Invoke(_lastClientHostVisibility, visible);
            _lastClientHostVisibility = visible;
        }

        /// <summary>
        /// If needed Renderers collection is initialized and populated.
        /// </summary>
        private void InitializeRendererCollection(bool force, bool updateVisibility)
        {
            if (!force && _renderersPopulated)
                return;

            List<Renderer> cache = CollectionCaches<Renderer>.RetrieveList();
            GetComponentsInChildren<Renderer>(includeInactive: true, cache);

            _renderers = new();

            foreach (Renderer r in cache)
            {
                if (r.enabled)
                    _renderers.Add(r);
            }

            CollectionCaches<Renderer>.Store(cache);

            /* Intentionally set before event call. This is to prevent
             * a potential endless loop should the user make another call
             * to this objects renderer API from the event, resulting in
             * the population repeating. */
            _renderersPopulated = true;

            if (updateVisibility)
                UpdateRenderVisibility(_lastClientHostVisibility);
        }

        /// <summary>
        /// Adds the default NetworkObserver conditions using the ObserverManager.
        /// </summary>
        private void AddDefaultNetworkObserverConditions()
        {
            if (_networkObserverInitiliazed)
                return;

            NetworkObserver = NetworkManager.ObserverManager.AddDefaultConditions(this);
        }

        internal void ClearObservers()
        {
            int startCount = Observers.Count;

            Observers.Clear();

            /* Done before ObserversActiveChanged because
             * that trickles into storing the LOD collection.
             * Realistically, the collection does not need to be
             * cleared because when it's stored it would be
             * then, but keeping this here for intent. */
            ClearObserverLevelOfDetail();
            
            if (startCount > 0)
                ObserversActiveChanged();
        }

        internal bool AddObserver(NetworkConnection connection)
        {
            int startCount = Observers.Count;

            bool added = Observers.Add(connection);

            if (added)
            {
                if (TimeManager != null)
                    ObserverAddedTick = TimeManager.LocalTick;

                if (startCount == 0)
                    ObserversActiveChanged();        
                
                /* Add after ObserversActiveChanged because
                * the LOD might not be setup yet otherwise;
                 * LOD uses hot loading. */
                AddObserverLevelOfDetail(connection);
            }

            return added;
        }

        /// <summary>
        /// Removes a connection from observers for this object returning if the connection was removed.
        /// </summary>
        /// <param name = "connection"></param>
        internal bool RemoveObserver(NetworkConnection connection)
        {
            RemoveObserverLevelOfDetail(connection);

            int startCount = Observers.Count;

            bool removed = Observers.Remove(connection);

            //Like with clear, remove before calling ObserversActiveChange.
            RemoveObserverLevelOfDetail(connection);
            
            if (removed && startCount == 0)
                ObserversActiveChanged();

            return removed;
        }

        /// <summary>
        /// Called when observers active has changed.
        /// </summary>
        private void ObserversActiveChanged()
        {
            ObserversActiveChanged_LevelOfDetail();
            OnObserversActive?.Invoke(this);
        }

        /// <summary>
        /// Adds the connection to observers if conditions are met.
        /// </summary>
        /// <param name = "connection"></param>
        /// <returns>True if added to Observers.</returns>
        internal ObserverStateChange RebuildObservers(NetworkConnection connection, bool timedOnly)
        {
            // If not a valid connection.
            if (!connection.IsValid)
            {
                NetworkManager.LogWarning($"An invalid connection was used when rebuilding observers.");
                return ObserverStateChange.Unchanged;
            }

            // Valid not not active.
            if (!connection.IsActive)
            {
                /* Just remove from observers since connection isn't active
                 * and return unchanged because nothing should process
                 * given the connection isnt active. */
                RemoveObserver(connection);
                return ObserverStateChange.Unchanged;
            }

            if (IsDeinitializing)
            {
                /* If object is deinitializing it's either being despawned
                 * this frame or it's not spawned. If we've made it this far,
                 * it's most likely being despawned. */
                return ObserverStateChange.Unchanged;
            }

            // Update hashgrid if needed.
            UpdateForNetworkObject(!timedOnly);

            ObserverStateChange osc = NetworkObserver.RebuildObservers(connection, timedOnly);

            if (osc == ObserverStateChange.Added)
                AddObserver(connection);
            else if (osc == ObserverStateChange.Removed)
                RemoveObserver(connection);

            return osc;
        }

        /// <summary>
        /// Resets this object to starting values.
        /// </summary>
        private void ResetState_Observers(bool asServer)
        {
            // As server or client it's safe to reset this value.
            ObserverAddedTick = TimeManager.UNSET_TICK;
        }
    }
}