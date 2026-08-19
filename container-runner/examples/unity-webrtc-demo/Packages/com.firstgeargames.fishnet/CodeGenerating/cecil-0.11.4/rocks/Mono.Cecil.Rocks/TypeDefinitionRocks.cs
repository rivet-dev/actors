//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;
using System.Collections.Generic;
using System.Linq;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        static class TypeDefinitionRocks
    {
        public static IEnumerable<MethodDefinition> GetConstructors(this TypeDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            if (!self.HasMethods)
                return Empty<MethodDefinition>.Array;

            return self.Methods.Where(method => method.IsConstructor);
        }

        public static MethodDefinition GetStaticConstructor(this TypeDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            if (!self.HasMethods)
                return null;

            return self.GetConstructors().FirstOrDefault(ctor => ctor.IsStatic);
        }

        public static IEnumerable<MethodDefinition> GetMethods(this TypeDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");

            if (!self.HasMethods)
                return Empty<MethodDefinition>.Array;

            return self.Methods.Where(method => !method.IsConstructor);
        }

        public static TypeReference GetEnumUnderlyingType(this TypeDefinition self)
        {
            if (self == null)
                throw new ArgumentNullException("self");
            if (!self.IsEnum)
                throw new ArgumentException();

            return Mixin.GetEnumUnderlyingType(self);
        }
    }
}