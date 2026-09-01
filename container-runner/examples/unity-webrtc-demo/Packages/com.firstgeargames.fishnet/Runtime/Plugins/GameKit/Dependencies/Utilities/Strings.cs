using System;
using System.Text;

namespace GameKit.Dependencies.Utilities
{
    public static class Strings
    {
        /// <summary>
        /// Used to encode and decode strings.
        /// </summary>
        private static readonly UTF8Encoding _encoding = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);
        /// <summary>
        /// A buffer convert data and discard.
        /// </summary>
        public static byte[] Buffer = new byte[1024];

        /// <summary>
        /// Converts a member string text to PascalCase
        /// </summary>
        /// <remarks>A member string is expected to be in the format '_memberName'.</remarks>
        public static string MemberToPascalCase(this string txt)
        {
            if (txt.Length < 2)
            {
                UnityEngine.Debug.LogError($"Text '{txt}' is too short.");
                return string.Empty;
            }

            if (txt[0] != '_')
            {
                UnityEngine.Debug.LogError($"Text '{txt}' has the incorrect member prefix.");
                return string.Empty;
            }

            string firstLeter = txt[1].ToString().ToUpper();

            string substring = txt.Length > 2 ? txt.Substring(2) : string.Empty;
            return $"{firstLeter}{substring}";
        }

        /// <summary>
        /// Converts a pascal case string to member case.
        /// </summary>
        /// <remarks>A PascalCase string is expected to be in the format 'PropertyName'.</remarks>
        public static string PascalCaseToMember(this string txt)
        {
            if (txt.Length < 1)
            {
                UnityEngine.Debug.LogError($"Text '{txt}' is too short.");
                return string.Empty;
            }

            string firstLeter = txt[0].ToString().ToLower();

            string subString = txt.Length > 1 ? txt.Substring(1) : string.Empty;
            return $"_{firstLeter}{subString}";
        }

        /// <summary>
        /// Attachs or detaches an suffix to a string.
        /// </summary>
        /// <param name = "text"></param>
        /// <param name = "suffix"></param>
        /// <param name = "addExtension"></param>
        public static string ReturnModifySuffix(string text, string suffix, bool addExtension)
        {
            /* Since saving to a json, add the .json extension if not present.
             * Length must be greater than 6 to contain a character and .json. */
            if (text.Length > suffix.Length + 1)
            {
                // If to add the extension.
                if (addExtension)
                {
                    // If doesn't contain the extension then add it on.
                    if (!text.Substring(text.Length - suffix.Length).Contains(suffix, StringComparison.CurrentCultureIgnoreCase))
                        return text + suffix;
                    // Already contains extension.
                    else
                        return text;
                }
                // Remove extension.
                else
                {
                    // If contains extension.
                    if (text.Substring(text.Length - suffix.Length).Contains(suffix, StringComparison.CurrentCultureIgnoreCase))
                        return text.Substring(0, text.Length - suffix.Length);
                    // Doesn't contain extension.
                    return text;
                }
            }
            // Text isn't long enough to manipulate.
            else
            {
                return text;
            }
        }

        /// <summary>
        /// Converts a string into a byte array buffer.
        /// </summary>
        /// <returns>Number of bytes written to the buffer.</returns>
        public static int ToBytes(this string value, ref byte[] buffer)
        {
            int strLength = value.Length;
            // Number of minimum bytes the buffer must be.
            int bytesNeeded = _encoding.GetMaxByteCount(strLength);

            // Grow string buffer if needed.
            if (buffer.Length < bytesNeeded)
                Array.Resize(ref buffer, bytesNeeded * 2);

            return _encoding.GetBytes(value, 0, strLength, buffer, 0);
        }

        /// <summary>
        /// Converts a string to bytes while allocating.
        /// </summary>
        public static byte[] ToBytesAllocated(this string value) => Encoding.Unicode.GetBytes(value);
    }
}