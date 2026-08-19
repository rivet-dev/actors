using System.Collections.Generic;
using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Transforms
    {
        /// <summary>
        /// Returns the sizeDelta halfed.
        /// </summary>
        /// <param name = "considerScale">True to multiple values by RectTransform scale.</param>
        public static Vector2 HalfSizeDelta(this RectTransform rectTransform, bool useScale = false)
        {
            Vector2 sizeDelta = useScale ? rectTransform.SizeDeltaScaled() : rectTransform.sizeDelta;
            return sizeDelta / 2f;
        }

        /// <summary>
        /// Returns the sizeDelta multiplied by scale.
        /// </summary>
        public static Vector2 SizeDeltaScaled(this RectTransform rectTransform)
        {
            return rectTransform.sizeDelta * rectTransform.localScale;
        }

        /// <summary>
        /// Returns a position for the rectTransform ensuring it's fully on the screen.
        /// </summary>
        /// <param name = "desiredPosition">Preferred position for the rectTransform.</param>
        /// <param name = "padding">How much padding the transform must be from the screen edges.</param>
        public static Vector3 GetOnScreenPosition(this RectTransform rectTransform, Vector3 desiredPosition, Vector2 padding)
        {
            RectTransform canvasRectTransform = rectTransform.GetComponentInParent<Canvas>().transform as RectTransform;
            Vector2 clampedPos = desiredPosition;
            Vector2 localScale = canvasRectTransform.localScale;
            Vector2 oneMinusPivot = Vector2.one - rectTransform.pivot;

            // The size has to be scaled to account for the size and scale of the Canvas it is childed to
            Vector2 scaledSize = rectTransform.sizeDelta * localScale;

            // Calculate the minimum and maximum bounds of the canvas our object can occupy
            Vector2 minClamp = scaledSize * rectTransform.pivot + padding;
            Vector2 maxClamp = (canvasRectTransform.rect.size - (rectTransform.sizeDelta * oneMinusPivot + padding)) * localScale;

            float clampX = Mathf.Clamp(clampedPos.x, minClamp.x, maxClamp.x);
            float clampY = Mathf.Clamp(clampedPos.y, minClamp.y, maxClamp.y);

            return new Vector2(clampX, clampY);
        }

        /// <summary>
        /// Sets a parent for src while maintaining position, rotation, and scale of src.
        /// </summary>
        /// <param name = "parent">Transform to become a child of.</param>
        public static void SetParentAndKeepTransform(this Transform src, Transform parent)
        {
            Vector3 pos = src.position;
            Quaternion rot = src.rotation;
            Vector3 scale = src.localScale;

            src.SetParent(parent);
            src.position = pos;
            src.rotation = rot;
            src.localScale = scale;
        }

        /// <summary>
        /// Destroys all children under the specified transform.
        /// </summary>
        public static void DestroyChildren(this Transform t, bool destroyImmediately = false)
        {
            // If destroying immediately then the iteration needs to occur only on the top-most children.
            if (destroyImmediately)
            {
                List<Transform> children = CollectionCaches<Transform>.RetrieveList();
                int childCount = t.childCount;

                for (int i = 0; i < childCount; i++)
                    children.Add(t.GetChild(i));

                foreach (Transform child in children)
                    Object.DestroyImmediate(child);

                CollectionCaches<Transform>.Store(children);
            }
            // Iterate using Unitys enumerator.
            else
            {
                foreach (Transform child in t)
                    Object.Destroy(child.gameObject);
            }
        }

        /// <summary>
        /// Destroys all children of a type under the specified transform.
        /// </summary>
        public static void DestroyChildren<T>(this Transform t, bool destroyImmediately = false) where T : MonoBehaviour
        {
            T[] children = t.GetComponentsInChildren<T>();
            foreach (T child in children)
            {
                if (destroyImmediately)
                    MonoBehaviour.DestroyImmediate(child.gameObject);
                else
                    Object.Destroy(child.gameObject);
            }
        }

        /// <summary>
        /// Gets components in children and optionally parent.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "results"></param>
        /// <param name = "parent"></param>
        /// <param name = "includeParent"></param>
        /// <param name = "includeInactive"></param>
        public static void GetComponentsInChildren<T>(this Transform parent, List<T> results, bool includeParent = true, bool includeInactive = false) where T : Component
        {
            if (!includeParent)
            {
                List<T> current = CollectionCaches<T>.RetrieveList();
                for (int i = 0; i < parent.childCount; i++)
                {
                    parent.GetChild(i).GetComponentsInChildren(includeInactive, current);
                    results.AddRange(current);
                }
                CollectionCaches<T>.Store(current);
            }
            else
            {
                parent.GetComponentsInChildren(includeInactive, results);
            }
        }

        /// <summary>
        /// Returns the position of this transform.
        /// </summary>
        public static Vector3 GetPosition(this Transform t, bool localSpace)
        {
            return localSpace ? t.localPosition : t.position;
        }

        /// <summary>
        /// Returns the rotation of this transform.
        /// </summary>
        public static Quaternion GetRotation(this Transform t, bool localSpace)
        {
            return localSpace ? t.localRotation : t.rotation;
        }

        /// <summary>
        /// Returns the scale of this transform.
        /// </summary>
        public static Vector3 GetScale(this Transform t)
        {
            return t.localScale;
        }

        /// <summary>
        /// Sets the position of this transform.
        /// </summary>
        /// <param name = "t"></param>
        /// <param name = "localSpace"></param>
        public static void SetPosition(this Transform t, bool localSpace, Vector3 pos)
        {
            if (localSpace)
                t.localPosition = pos;
            else
                t.position = pos;
        }

        /// <summary>
        /// Sets the position of this transform.
        /// </summary>
        /// <param name = "t"></param>
        /// <param name = "localSpace"></param>
        public static void SetRotation(this Transform t, bool localSpace, Quaternion rot)
        {
            if (localSpace)
                t.localRotation = rot;
            else
                t.rotation = rot;
        }

        /// <summary>
        /// Sets the position of this transform.
        /// </summary>
        /// <param name = "t"></param>
        /// <param name = "localSpace"></param>
        public static void SetScale(this Transform t, Vector3 scale)
        {
            t.localScale = scale;
        }
    }
}