using System.Collections.Generic;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Layers
    {
        /* GetInteractableLayer methods is an implementation from this
         * link: https://forum.unity.com/threads/is-there-a-way-to-get-the-layer-collision-matrix.260744/#post-3483886 */
        /// <summary>
        /// Lookup of interactable layers for each layer.
        /// </summary>
        private static Dictionary<int, int> _interactablesLayers;

        /// <summary>
        /// Tries to initializes InteractableLayers.
        /// </summary>
        private static void TryInitializeInteractableLayers()
        {
            if (_interactablesLayers != null)
                return;

            _interactablesLayers = new();
            for (int i = 0; i < 32; i++)
            {
                int mask = 0;
                for (int j = 0; j < 32; j++)
                {
                    if (!Physics.GetIgnoreLayerCollision(i, j))
                    {
                        mask |= 1 << j;
                    }
                }
                // Setting without add check is quicker.
                _interactablesLayers[i] = mask;
            }
        }

        /// <summary>
        /// Returns interactable layers value for layer.
        /// </summary>
        public static int GetInteractableLayersValue(int layer)
        {
            TryInitializeInteractableLayers();
            return _interactablesLayers[layer];
        }

        /// <summary>
        /// Returns interactable layers LayerMask for a GameObject.
        /// </summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static LayerMask GetInteractableLayersMask(int layer) => (LayerMask)GetInteractableLayersValue(layer);

        /// <summary>
        /// Returns interactable layers value for a GameObject.
        /// </summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static int GetInteractableLayersValue(GameObject go) => GetInteractableLayersValue(go.layer);

        /// <summary>
        /// Returns interactable layers LayerMask for a GameObject.
        /// </summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static LayerMask GetInteractableLayersMask(GameObject go) => (LayerMask)GetInteractableLayersValue(go.layer);

        /// <summary>
        /// Converts a layer mask to a layer number.
        /// </summary>
        /// <param name = "mask"></param>
        /// <returns></returns>
        public static int LayerMaskToLayerNumber(LayerMask mask)
        {
            return LayerValueToLayerNumber(mask.value);
        }

        /// <summary>
        /// Converts a layer value int to a layer int.
        /// </summary>
        /// <param name = "bitmask"></param>
        /// <returns></returns>
        public static int LayerValueToLayerNumber(int bitmask)
        {
            int result = bitmask > 0 ? 0 : 31;
            while (bitmask > 1)
            {
                bitmask = bitmask >> 1;
                result++;
            }
            return result;
        }

        /// <summary>
        /// Returns if a LayerMask contains a specified layer.
        /// </summary>
        /// <param name = "layerMask">LayerMask to check for layer in.</param>
        /// <param name = "layer">Layer to check within LayerMask.</param>
        /// <returns></returns>
        public static bool ContainsLayer(LayerMask layerMask, int layer)
        {
            return layerMask == (layerMask | (1 << layer));
        }
    }
}