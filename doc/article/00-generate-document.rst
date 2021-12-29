============================================
Meta-instruction on generating this document
============================================

The document can be built locally, within the same development environment 
described in project's README. Additional required apt packages:

.. code-block:: text

    libclang1-9 libclang-cpp9 python3-sphinx python3-pip

The following pypi packages are also required:

.. code-block:: text

    furo m2r2

Although there are also packages for Doxygen and Breathe, we need too recent
version of them compare to the one presents in Ubuntu 21.10. The Doxygen can be
downloaded `here`_, and Breathe as submodule can be found in ``dependency``
directory. Make sure to put ``doxygen`` binary in path, and run

.. code-block:: bash

    python3 setup.py install --user

In ``dependency/breathe`` directory before continuing.

.. _here: https://www.doxygen.nl/files/doxygen-1.9.2.linux.bin.tar.gz

----

After setting up, in project directory run:

.. code-block:: bash

    doxygen
    sphinx-build -M html doc doc/_build

The built document (identical to gh-pages branch content) is located in 
``doc/_build/html``.
