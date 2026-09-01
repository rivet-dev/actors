using System.Collections.Generic;
using System.Text;
using CodeBoost.Extensions;

namespace CodeBoost.Performance
{

    /// <summary>
    /// A pool for a type which is not resettable.
    /// </summary>
    public static class Utf8EncodingPool
    {
        /// <summary>
        /// Stack to use.
        /// </summary>
        private static readonly Stack<UTF8Encoding> Stack = new();

        /// <summary>
        /// Returns a value from the stack or creates an instance when the stack is empty.
        /// </summary>
        /// <returns> </returns>
        public static UTF8Encoding Rent()
        {
            if (!Stack.TryPop(out UTF8Encoding result))
                result = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);
                
            return result;
        }

        /// <summary>
        /// Stores an instance of T0 and sets the original reference to default.
        /// Method will not execute if value is null.
        /// </summary>
        /// <param name = "value"> Value to return. </param>
        public static void ReturnAndNullifyReference(ref UTF8Encoding value)
        {
            Return(value);

            value = null;
        }

        /// <summary>
        /// Stores a value to the stack.
        /// </summary>
        /// <param name = "value"> </param>
        public static void Return(UTF8Encoding value)
        {
            if (value is null)
                return;

            Stack.Push(value);
        }
    }
}
